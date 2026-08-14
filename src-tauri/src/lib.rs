use escpos::driver::{Driver, NativeUsbDriver};
#[cfg(target_os = "windows")]
use escpos::driver::WindowsUsbPrintDriver;
use escpos::printer::Printer;
use escpos::utils::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{Emitter, Manager, State};
use axum::{routing::post, Json, Router, http::StatusCode, http::Method};
use tower_http::cors::{Any, CorsLayer};

const BRIDGE_PORT: u16 = 9876;

// =========================================================================
// 1. MODELOS Y CONFIGURACIÓN
// =========================================================================

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub enum PaperSize {
    Size60mm,
    Size80mm,
}

impl PaperSize {
    fn chars_per_line(&self) -> usize {
        match self {
            PaperSize::Size60mm => 48,
            PaperSize::Size80mm => 64,
        }
    }

    fn separator(&self) -> String {
        "=".repeat(self.chars_per_line())
    }

    fn format_two_cols(&self, left: &str, right: &str) -> String {
        let total_width = self.chars_per_line();
        let left_width = (total_width - right.len()).saturating_sub(1);
        format!("{:<width$} {}", left, right, width = left_width)
    }

    /// Formatea fila de 4 columnas: Item, Precio, Cant, Monto
    fn format_table_row(&self, item: &str, price: &str, qty: &str, amount: &str) -> String {
        let total_width = self.chars_per_line();
        let price_width = 10;
        let qty_width = 8;
        let amount_width = 10;
        let item_width = total_width.saturating_sub(price_width + qty_width + amount_width);

        format!(
            "{:<item_w$}{:>price_w$}{:>qty_w$}{:>amount_w$}",
            item,
            price,
            qty,
            amount,
            item_w = item_width,
            price_w = price_width,
            qty_w = qty_width,
            amount_w = amount_width,
        )
    }

    fn format_table_header(&self) -> String {
        self.format_table_row("Item", "Precio", "Cant", "Monto")
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UsbPrinter {
    vendor_id: u16,
    product_id: u16,
    device_path: Option<String>,
    name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PrinterConfig {
    vendor_id: Option<u16>,
    product_id: Option<u16>,
    device_path: Option<String>,
    paper_size: PaperSize,
}

impl Default for PrinterConfig {
    fn default() -> Self {
        Self {
            vendor_id: None,
            product_id: None,
            device_path: None,
            paper_size: PaperSize::Size80mm,
        }
    }
}

// =========================================================================
// 2. ESTRUCTURAS UNIVERSALES PARA TICKETS (formato JSON universal)
// =========================================================================

#[derive(Deserialize)]
pub struct TicketPayload {
    /// Nombre de la tienda (se muestra como encabezado principal)
    pub store_name: String,
    /// URL opcional del logotipo para imprimir
    pub logo_url: Option<String>,
    /// Líneas de texto antes de los artículos
    #[serde(default)]
    pub text_lines_before_items: Vec<TextLine>,
    /// Lista de artículos/items
    pub items: Vec<TicketItem>,
    /// Subtotal (opcional)
    pub subtotal: Option<f64>,
    /// IVA / Impuesto (opcional)
    pub iva: Option<f64>,
    /// Total de la venta
    pub total: f64,
    /// Líneas de texto después de totales (subtotal/iva/total).
    #[serde(default)]
    pub text_lines_after_items: Vec<TextLine>,
    /// Código de barras o QR opcional
    pub barcode: Option<BarcodeInfo>,
}

#[derive(Deserialize)]
pub struct TicketItem {
    pub name: String,
    pub price: f64,
    pub qty: f64,
}

#[derive(Deserialize)]
pub struct TextLine {
    /// Texto de la etiqueta (opcional)
    pub label: Option<String>,
    /// Si la etiqueta debe ir en negrita
    #[serde(default)]
    pub label_bold: bool,
    /// Valor/texto (opcional)
    pub value: Option<String>,
    /// Si el valor debe ir en negrita
    #[serde(default)]
    pub value_bold: bool,
    /// Tamaño de fuente (12 = normal)
    #[serde(default = "default_font_size")]
    pub font_size: u32,
    /// Alineación del texto
    #[serde(default)]
    pub alignment: TextAlignment,
}

fn default_font_size() -> u32 {
    12
}

#[derive(Deserialize, Default)]
pub enum TextAlignment {
    #[default]
    #[serde(rename = "left")]
    Left,
    #[serde(rename = "right")]
    Right,
    #[serde(rename = "center")]
    Center,
    #[serde(rename = "space_between")]
    SpaceBetween,
}

#[derive(Deserialize)]
pub struct BarcodeInfo {
    /// Tipo: "bar" para código de barras, "qr" para código QR
    #[serde(rename = "type")]
    pub barcode_type: BarcodeType,
    /// Valor/datos del código
    pub value: String,
}

#[derive(Deserialize)]
pub enum BarcodeType {
    #[serde(rename = "bar")]
    Bar,
    #[serde(rename = "qr")]
    Qr,
}

// Estado global de Tauri protegido por un Mutex para concurrencia segura
pub struct AppConfigState {
    config: Mutex<Option<PrinterConfig>>,
}

// Estado del servidor Bridge HTTP (Axum)
pub struct BridgeState {
    running: Arc<AtomicBool>,
    shutdown_tx: Arc<Mutex<Option<tokio::sync::watch::Sender<bool>>>>,
}

// Registro individual del historial de impresión
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PrintLogEntry {
    timestamp: String,
    source: String,       // "Bridge HTTP" o "Prueba local"
    store_name: String,
    total: String,
    item_count: usize,
    success: bool,
}

// Historial de impresión compartido
pub struct PrintHistory {
    logs: Arc<Mutex<Vec<PrintLogEntry>>>,
}

// =========================================================================
// 3. PERSISTENCIA DE CONFIGURACIÓN EN DISCO (JSON)
// =========================================================================

fn get_config_file_path(app_handle: &tauri::AppHandle) -> Result<PathBuf, String> {
    let app_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("No se pudo obtener el directorio app_data: {}", e))?;

    if !app_dir.exists() {
        let _ = fs::create_dir_all(&app_dir);
    }
    Ok(app_dir.join("printer_config.json"))
}

fn load_config_from_disk(app_handle: &tauri::AppHandle) -> PrinterConfig {
    if let Ok(path) = get_config_file_path(app_handle) {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(config) = serde_json::from_str::<PrinterConfig>(&content) {
                    return config;
                }
            }
        }
    }
    PrinterConfig::default()
}

fn save_config_to_disk(app_handle: &tauri::AppHandle, config: &PrinterConfig) -> Result<(), String> {
    let path = get_config_file_path(app_handle)?;
    let json_str = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Error al serializar la configuración: {}", e))?;
    fs::write(path, json_str).map_err(|e| format!("Error al escribir archivo de configuración: {}", e))?;
    Ok(())
}

// =========================================================================
// 4. DRIVER DE IMPRESIÓN DINÁMICO (Soporte Linux + Windows)
// =========================================================================

pub struct DynamicDriver(Box<dyn Driver + Send + Sync>);

impl Driver for DynamicDriver {
    fn name(&self) -> String {
        self.0.name()
    }
    fn write(&self, data: &[u8]) -> escpos::errors::Result<()> {
        self.0.write(data)
    }
    fn read(&self, buf: &mut [u8]) -> escpos::errors::Result<usize> {
        self.0.read(buf)
    }
    fn flush(&self) -> escpos::errors::Result<()> {
        self.0.flush()
    }
}

fn open_printer_driver(config: &PrinterConfig) -> Result<DynamicDriver, String> {
    #[cfg(target_os = "windows")]
    {
        // 1. Si tenemos un device_path específico para Windows, intentamos usar WindowsUsbPrintDriver
        if let Some(ref path) = config.device_path {
            if let Ok(driver) = WindowsUsbPrintDriver::open(path) {
                return Ok(DynamicDriver(Box::new(driver)));
            }
        }

        // 2. Si tenemos VID y PID, intentamos conectar por WindowsUsbPrintDriver primero (usbprint.sys)
        if let (Some(vid), Some(pid)) = (config.vendor_id, config.product_id) {
            if let Ok(driver) = WindowsUsbPrintDriver::open_by_vid_pid(vid, pid) {
                return Ok(DynamicDriver(Box::new(driver)));
            }

            // 3. Fallback a NativeUsbDriver (WinUSB / libusb) en Windows
            if let Ok(driver) = NativeUsbDriver::open(vid, pid) {
                return Ok(DynamicDriver(Box::new(driver)));
            }
        }

        Err("No se pudo conectar a la impresora en Windows. Verifica la conexión USB y que la impresora esté encendida.".to_string())
    }

    #[cfg(not(target_os = "windows"))]
    {
        if let (Some(vid), Some(pid)) = (config.vendor_id, config.product_id) {
            let driver = NativeUsbDriver::open(vid, pid)
                .map_err(|e| format!("Error al abrir la impresora USB (VID:{:04x}, PID:{:04x}): {}", vid, pid, e))?;
            Ok(DynamicDriver(Box::new(driver)))
        } else {
            Err("No se ha configurado el Vendor ID y Product ID de la impresora.".to_string())
        }
    }
}

// =========================================================================
// 5. COMANDOS DE TAURI (Para la UI en Vue.js)
// =========================================================================

#[tauri::command]
fn list_printers() -> Result<Vec<UsbPrinter>, String> {
    let mut printers = Vec::new();
    let mut seen_keys = std::collections::HashSet::new();

    // 1. En Windows, listar impresoras vía usbprint.sys nativo
    #[cfg(target_os = "windows")]
    {
        if let Ok(win_devices) = WindowsUsbPrintDriver::list() {
            for dev in win_devices {
                let vid = dev.vendor_id.unwrap_or(0);
                let pid = dev.product_id.unwrap_or(0);
                let key = format!("{:04x}:{:04x}:{}", vid, pid, dev.device_path);
                if seen_keys.contains(&key) {
                    continue;
                }
                seen_keys.insert(key);

                let name = format!("Impresora Windows USB [{:04x}:{:04x}]", vid, pid);
                printers.push(UsbPrinter {
                    vendor_id: vid,
                    product_id: pid,
                    device_path: Some(dev.device_path),
                    name,
                });
            }
        }
    }

    // 2. Listar mediante rusb (Linux y Windows)
    if let Ok(devices) = rusb::devices() {
        for device in devices.iter() {
            if let Ok(desc) = device.device_descriptor() {
                if !is_printer_device(&device, &desc) {
                    continue;
                }

                let vendor = desc.vendor_id();
                let product = desc.product_id();
                let key = format!("{:04x}:{:04x}", vendor, product);
                if seen_keys.contains(&key) {
                    continue;
                }
                seen_keys.insert(key);

                let name = match get_device_name(&device, &desc) {
                    Some(n) => n,
                    None => format!("Dispositivo USB [{:04x}:{:04x}]", vendor, product),
                };

                printers.push(UsbPrinter {
                    vendor_id: vendor,
                    product_id: product,
                    device_path: None,
                    name,
                });
            }
        }
    }

    Ok(printers)
}

/// Determina si un dispositivo USB es una impresora.
fn is_printer_device(
    device: &rusb::Device<rusb::GlobalContext>,
    desc: &rusb::DeviceDescriptor,
) -> bool {
    const USB_CLASS_PRINTER: u8 = 7;

    if desc.class_code() == USB_CLASS_PRINTER {
        return true;
    }

    if let Ok(config_desc) = device.config_descriptor(0) {
        for interface in config_desc.interfaces() {
            for iface_desc in interface.descriptors() {
                if iface_desc.class_code() == USB_CLASS_PRINTER {
                    return true;
                }
            }
        }
    }

    false
}

/// Intenta leer el manufacturer y product name desde los string descriptors USB.
fn get_device_name(device: &rusb::Device<rusb::GlobalContext>, desc: &rusb::DeviceDescriptor) -> Option<String> {
    if let Some(name) = try_libusb_name(device, desc) {
        return Some(name);
    }
    if let Some(name) = try_sysfs_name(desc.vendor_id(), desc.product_id()) {
        return Some(name);
    }
    None
}

fn try_libusb_name(device: &rusb::Device<rusb::GlobalContext>, desc: &rusb::DeviceDescriptor) -> Option<String> {
    let handle = device.open().ok()?;
    let timeout = std::time::Duration::from_millis(500);

    let langs = handle.read_languages(timeout).ok()?;
    let lang = langs.first()?;

    let manu = handle
        .read_manufacturer_string(*lang, desc, timeout)
        .unwrap_or_default()
        .trim()
        .to_string();

    let prod = handle
        .read_product_string(*lang, desc, timeout)
        .unwrap_or_default()
        .trim()
        .to_string();

    match (manu.is_empty(), prod.is_empty()) {
        (false, false) => Some(format!("{} {}", manu, prod)),
        (false, true) => Some(manu),
        (true, false) => Some(prod),
        (true, true) => None,
    }
}

fn try_sysfs_name(vendor_id: u16, product_id: u16) -> Option<String> {
    let sysfs_dir = std::path::Path::new("/sys/bus/usb/devices");
    let entries = std::fs::read_dir(sysfs_dir).ok()?;

    let target_vendor = format!("{:04x}", vendor_id);
    let target_product = format!("{:04x}", product_id);

    for entry in entries.flatten() {
        let dev_path = entry.path();

        let vendor_file = dev_path.join("idVendor");
        if let Ok(v) = std::fs::read_to_string(&vendor_file) {
            if v.trim() != target_vendor {
                continue;
            }
        } else {
            continue;
        }

        let product_file = dev_path.join("idProduct");
        if let Ok(p) = std::fs::read_to_string(&product_file) {
            if p.trim() != target_product {
                continue;
            }
        } else {
            continue;
        }

        let manu = std::fs::read_to_string(dev_path.join("manufacturer"))
            .unwrap_or_default()
            .trim()
            .to_string();

        let prod = std::fs::read_to_string(dev_path.join("product"))
            .unwrap_or_default()
            .trim()
            .to_string();

        return match (manu.is_empty(), prod.is_empty()) {
            (false, false) => Some(format!("{} {}", manu, prod)),
            (false, true) => Some(manu),
            (true, false) => Some(prod),
            (true, true) => None,
        };
    }

    None
}

#[tauri::command]
fn get_config(
    app_handle: tauri::AppHandle,
    state: State<'_, AppConfigState>,
) -> Result<PrinterConfig, String> {
    let mut guard = state.config.lock().unwrap();
    if guard.is_none() {
        let loaded = load_config_from_disk(&app_handle);
        *guard = Some(loaded.clone());
        Ok(loaded)
    } else {
        Ok(guard.clone().unwrap())
    }
}

#[tauri::command]
fn save_config(
    app_handle: tauri::AppHandle,
    vendor_id: Option<u16>,
    product_id: Option<u16>,
    device_path: Option<String>,
    paper_size: PaperSize,
    state: State<'_, AppConfigState>,
) -> Result<(), String> {
    let config = PrinterConfig {
        vendor_id,
        product_id,
        device_path,
        paper_size,
    };

    // Guardar en disco (JSON)
    save_config_to_disk(&app_handle, &config)?;

    // Actualizar estado en memoria
    let mut lock = state.config.lock().unwrap();
    *lock = Some(config);

    Ok(())
}

#[tauri::command]
fn print_test_page(
    app_handle: tauri::AppHandle,
    vendor_id: Option<u16>,
    product_id: Option<u16>,
    device_path: Option<String>,
    paper_size: PaperSize,
) -> Result<(), String> {
    let config = PrinterConfig {
        vendor_id,
        product_id,
        device_path,
        paper_size,
    };

    let driver = open_printer_driver(&config)?;
    let mut printer = Printer::new(driver, Default::default(), None);

    let result = printer
        .init().map_err(|e| e.to_string())?
        .justify(JustifyMode::CENTER).map_err(|e| e.to_string())?
        .size(2, 2).map_err(|e| e.to_string())?
        .bold(true).map_err(|e| e.to_string())?
        .writeln("HOJA DE PRUEBA").map_err(|e| e.to_string())?
        .bold(false).map_err(|e| e.to_string())?
        .reset_size().map_err(|e| e.to_string())?
        .writeln("Bridge").map_err(|e| e.to_string())?
        .feed().map_err(|e| e.to_string())?
        .justify(JustifyMode::LEFT).map_err(|e| e.to_string())?
        .writeln(&paper_size.separator()).map_err(|e| e.to_string())?
        .writeln("El puente de impresion local esta activo.").map_err(|e| e.to_string())?
        .writeln(&paper_size.separator()).map_err(|e| e.to_string())?
        .feed().map_err(|e| e.to_string())?
        .feed().map_err(|e| e.to_string())?
        .print_cut().map_err(|e| e.to_string());

    let log = PrintLogEntry {
        timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        source: "🧪 Prueba local".into(),
        store_name: "HOJA DE PRUEBA".into(),
        total: "$ —".into(),
        item_count: 0,
        success: result.is_ok(),
    };

    push_log_and_emit(&app_handle, log);

    result.map(|_| ()).map_err(|e| e.to_string())
}

/// Imprime un ticket de prueba completo con el nuevo formato universal
#[tauri::command]
async fn print_test_ticket(
    app_handle: tauri::AppHandle,
    vendor_id: Option<u16>,
    product_id: Option<u16>,
    device_path: Option<String>,
    paper_size: PaperSize,
) -> Result<(), String> {
    let config = PrinterConfig {
        vendor_id,
        product_id,
        device_path,
        paper_size,
    };

    let payload = TicketPayload {
        store_name: "Mi Tienda de Prueba".to_string(),
        logo_url: None,
        text_lines_before_items: vec![
            TextLine {
                label: Some("RFC: ".to_string()),
                label_bold: true,
                value: Some("XAXX010101000".to_string()),
                value_bold: true,
                font_size: 12,
                alignment: TextAlignment::SpaceBetween,
            },
            TextLine {
                label: Some("Dirección: ".to_string()),
                label_bold: false,
                value: Some("Av. Principal #123, Centro".to_string()),
                value_bold: false,
                font_size: 10,
                alignment: TextAlignment::Left,
            },
            TextLine {
                label: Some("Atendió: ".to_string()),
                label_bold: false,
                value: Some("Juan Pérez".to_string()),
                value_bold: false,
                font_size: 10,
                alignment: TextAlignment::Left,
            },
        ],
        items: vec![
            TicketItem {
                name: "Café Americano".to_string(),
                price: 45.00,
                qty: 2.0,
            },
            TicketItem {
                name: "Croissant".to_string(),
                price: 30.00,
                qty: 1.0,
            },
            TicketItem {
                name: "Jugo de Naranja".to_string(),
                price: 25.50,
                qty: 1.0,
            },
        ],
        subtotal: Some(145.50),
        iva: Some(23.28),
        total: 168.78,
        text_lines_after_items: vec![
            TextLine {
                label: Some("Forma de pago: ".to_string()),
                label_bold: false,
                value: Some("Tarjeta de crédito".to_string()),
                value_bold: false,
                font_size: 10,
                alignment: TextAlignment::Left,
            },
            TextLine {
                label: Some("¡Gracias por su compra!".to_string()),
                label_bold: false,
                value: None,
                value_bold: false,
                font_size: 14,
                alignment: TextAlignment::Center,
            },
        ],
        barcode: Some(BarcodeInfo {
            barcode_type: BarcodeType::Qr,
            value: "https://ejemplo.com/ticket/12345".to_string(),
        }),
    };

    let result = execute_print(config, payload).await;

    let log = PrintLogEntry {
        timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        source: "🧪 Prueba completa".into(),
        store_name: "Mi Tienda de Prueba".into(),
        total: "$168.78".into(),
        item_count: 3,
        success: result.is_ok(),
    };

    push_log_and_emit(&app_handle, log);

    result
}

// =========================================================================
// 6. HELPER: IMPRESIÓN DE LOGO DESDE URL
// =========================================================================

async fn download_logo(url: &str) -> Option<tempfile::NamedTempFile> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?;
    let bytes = client.get(url).send().await.ok()?.bytes().await.ok()?;
    let tmp = tempfile::NamedTempFile::new().ok()?;
    std::fs::write(tmp.path(), &bytes).ok()?;
    Some(tmp)
}

// =========================================================================
// 7. HELPER: IMPRESIÓN DE UNA LÍNEA DE TEXTO (TextLine)
// =========================================================================

fn print_text_line(
    printer: &mut Printer<DynamicDriver>,
    p: &PaperSize,
    line: &TextLine,
) -> Result<(), String> {
    let justify = match line.alignment {
        TextAlignment::Left => JustifyMode::LEFT,
        TextAlignment::Right => JustifyMode::RIGHT,
        TextAlignment::Center => JustifyMode::CENTER,
        TextAlignment::SpaceBetween => JustifyMode::LEFT,
    };

    let size_mult = ((line.font_size as f64) / 12.0).round().max(1.0).min(8.0) as u8;

    let text = match (&line.label, &line.value) {
        (Some(label), Some(val)) if matches!(line.alignment, TextAlignment::SpaceBetween) => {
            p.format_two_cols(label, val)
        }
        (Some(label), Some(val)) => format!("{}{}", label, val),
        (Some(label), None) => label.clone(),
        (None, Some(val)) => val.clone(),
        (None, None) => String::new(),
    };

    if text.is_empty() {
        printer.writeln("").map_err(|e| e.to_string())?;
        return Ok(());
    }

    let needs_bold = line.label_bold || line.value_bold;
    if needs_bold {
        printer.bold(true).map_err(|e| e.to_string())?;
    }

    if size_mult > 1 {
        printer.size(size_mult, size_mult).map_err(|e| e.to_string())?;
    }

    printer
        .justify(justify)
        .map_err(|e| e.to_string())?
        .writeln(&text)
        .map_err(|e| e.to_string())?;

    if size_mult > 1 {
        printer.reset_size().map_err(|e| e.to_string())?;
    }
    if needs_bold {
        printer.bold(false).map_err(|e| e.to_string())?;
    }

    Ok(())
}

// =========================================================================
// 8. LÓGICA DE IMPRESIÓN CENTRALIZADA (formato universal)
// =========================================================================

async fn execute_print(config: PrinterConfig, payload: TicketPayload) -> Result<(), String> {
    let driver = open_printer_driver(&config)?;
    let mut printer = Printer::new(driver, Default::default(), None);
    let p = &config.paper_size;

    printer.init().map_err(|e| e.to_string())?;

    // LOGO (opcional)
    if let Some(logo_url) = &payload.logo_url {
        if let Some(tmp_file) = download_logo(logo_url).await {
            let path = tmp_file.path().to_str().unwrap_or("").to_string();
            if !path.is_empty() {
                if let Ok(img_opt) = BitImageOption::new(None, None, BitImageSize::Normal) {
                    let logo_result = printer
                        .justify(JustifyMode::CENTER)
                        .map_err(|e| e.to_string())?
                        .bit_image_option(&path, img_opt)
                        .map_err(|e| e.to_string());
                    if let Err(e) = logo_result {
                        eprintln!("Logo no se pudo imprimir (continuando): {}", e);
                    }
                    printer.feed().map_err(|e| e.to_string())?;
                }
            }
        }
    }

    // ENCABEZADO
    printer
        .justify(JustifyMode::CENTER)
        .map_err(|e| e.to_string())?
        .size(2, 2)
        .map_err(|e| e.to_string())?
        .bold(true)
        .map_err(|e| e.to_string())?
        .writeln(&payload.store_name)
        .map_err(|e| e.to_string())?
        .bold(false)
        .map_err(|e| e.to_string())?
        .reset_size()
        .map_err(|e| e.to_string())?;

    // LÍNEAS ANTES DE ARTÍCULOS
    for line in &payload.text_lines_before_items {
        print_text_line(&mut printer, p, line)?;
    }

    // TABLA DE ARTÍCULOS
    printer.feed().map_err(|e| e.to_string())?;
    printer
        .justify(JustifyMode::LEFT)
        .map_err(|e| e.to_string())?
        .writeln(&p.separator())
        .map_err(|e| e.to_string())?
        .writeln(&p.format_table_header())
        .map_err(|e| e.to_string())?
        .writeln(&p.separator())
        .map_err(|e| e.to_string())?;

    for item in &payload.items {
        let price_str = format!("${:.2}", item.price);
        let qty_str = format!("{:.2}", item.qty);
        let amount_str = format!("${:.2}", item.price * item.qty);
        printer
            .writeln(&p.format_table_row(&item.name, &price_str, &qty_str, &amount_str))
            .map_err(|e| e.to_string())?;
    }

    printer
        .writeln(&p.separator())
        .map_err(|e| e.to_string())?
        .feed()
        .map_err(|e| e.to_string())?;

    // SUBTOTAL E IVA
    if let Some(subtotal) = payload.subtotal {
        printer
            .justify(JustifyMode::RIGHT)
            .map_err(|e| e.to_string())?
            .writeln(&p.format_two_cols("Subtotal:", &format!("${:.2}", subtotal)))
            .map_err(|e| e.to_string())?;
    }
    if let Some(iva) = payload.iva {
        printer
            .justify(JustifyMode::RIGHT)
            .map_err(|e| e.to_string())?
            .writeln(&p.format_two_cols("IVA:", &format!("${:.2}", iva)))
            .map_err(|e| e.to_string())?;
    }

    // TOTAL
    let total_str = format!("${:.2}", payload.total);
    printer
        .justify(JustifyMode::RIGHT)
        .map_err(|e| e.to_string())?
        .size(2, 2)
        .map_err(|e| e.to_string())?
        .bold(true)
        .map_err(|e| e.to_string())?
        .writeln(&p.format_two_cols("TOTAL:", &total_str))
        .map_err(|e| e.to_string())?
        .bold(false)
        .map_err(|e| e.to_string())?
        .reset_size()
        .map_err(|e| e.to_string())?
        .feed()
        .map_err(|e| e.to_string())?;

    // LÍNEAS DESPUÉS DE TOTALES
    for line in &payload.text_lines_after_items {
        print_text_line(&mut printer, p, line)?;
    }

    // CÓDIGO DE BARRAS / QR
    if let Some(barcode) = &payload.barcode {
        printer.feed().map_err(|e| e.to_string())?;
        printer
            .justify(JustifyMode::CENTER)
            .map_err(|e| e.to_string())?;

        match barcode.barcode_type {
            BarcodeType::Bar => {
                let bar_result = printer
                    .code39_option(
                        &barcode.value,
                        BarcodeOption::new(
                            BarcodeWidth::M,
                            BarcodeHeight::M,
                            BarcodeFont::A,
                            BarcodePosition::Below,
                        ),
                    )
                    .map_err(|e| e.to_string());
                if let Err(e) = bar_result {
                    eprintln!("Código de barras no se pudo imprimir: {}", e);
                }
            }
            BarcodeType::Qr => {
                let qr_result = printer
                    .qrcode_option(
                        &barcode.value,
                        QRCodeOption::new(QRCodeModel::Model2, 6, QRCodeCorrectionLevel::M),
                    )
                    .map_err(|e| e.to_string());
                if let Err(e) = qr_result {
                    eprintln!("QR no se pudo imprimir: {}", e);
                }
            }
        }
    }

    // PIE DE PÁGINA Y CORTE
    printer
        .feed()
        .map_err(|e| e.to_string())?
        .justify(JustifyMode::CENTER)
        .map_err(|e| e.to_string())?
        .writeln("Gracias por su compra")
        .map_err(|e| e.to_string())?
        .feed()
        .map_err(|e| e.to_string())?
        .feed()
        .map_err(|e| e.to_string())?
        .print_cut()
        .map_err(|e| e.to_string())?;

    Ok(())
}

// =========================================================================
// 9. REGISTRO DE HISTORIAL DE IMPRESIÓN
// =========================================================================

fn push_log_and_emit(app_handle: &tauri::AppHandle, entry: PrintLogEntry) {
    let history = app_handle.state::<PrintHistory>();
    let mut logs = history.logs.lock().unwrap();
    logs.push(entry.clone());
    if logs.len() > 200 {
        let excess = logs.len() - 200;
        logs.drain(0..excess);
    }
    let _ = app_handle.emit("print-log", entry);
}

#[tauri::command]
fn get_print_history(state: State<'_, PrintHistory>) -> Result<Vec<PrintLogEntry>, String> {
    Ok(state.logs.lock().unwrap().clone())
}

// =========================================================================
// 10. MANEJADOR DEL SERVIDOR HTTP (AXUM)
// =========================================================================

async fn http_print_handler(
    axum::extract::State(handle): axum::extract::State<tauri::AppHandle>,
    Json(payload): Json<TicketPayload>,
) -> impl axum::response::IntoResponse {
    let store = payload.store_name.clone();
    let total_str = format!("${:.2}", payload.total);
    let count = payload.items.len();

    let printer_config = {
        let app_state = handle.state::<AppConfigState>();
        let guard = app_state.config.lock().unwrap();
        guard.clone()
    };

    let (status, msg) = if let Some(config) = printer_config {
        match execute_print(config, payload).await {
            Ok(_) => (StatusCode::OK, "Ticket impreso correctamente".to_string()),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Error de hardware: {}", e)),
        }
    } else {
        (StatusCode::BAD_REQUEST, "Error: Primero debes configurar y guardar una impresora en la interfaz de Tauri.".to_string())
    };

    let success = status.is_success();
    let log = PrintLogEntry {
        timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        source: "🌐 Bridge HTTP".into(),
        store_name: store,
        total: total_str,
        item_count: count,
        success,
    };

    push_log_and_emit(&handle, log);

    (status, msg)
}

// =========================================================================
// 11. GESTIÓN DEL SERVIDOR BRIDGE (inicio / reinicio / estado)
// =========================================================================

fn spawn_bridge(
    app_handle: tauri::AppHandle,
    running: Arc<AtomicBool>,
    shutdown_tx_cell: Arc<Mutex<Option<tokio::sync::watch::Sender<bool>>>>,
) {
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    *shutdown_tx_cell.lock().unwrap() = Some(shutdown_tx);

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("failed to create Tokio runtime");
        rt.block_on(async move {
            running.store(true, Ordering::SeqCst);
            let _ = app_handle.emit("bridge-status", true);

            let cors = CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([Method::POST])
                .allow_headers(Any);

            let routes = Router::new()
                .route("/print", post(http_print_handler))
                .layer(cors)
                .with_state(app_handle.clone());

            let addr = format!("127.0.0.1:{}", BRIDGE_PORT);
            let listener = match tokio::net::TcpListener::bind(&addr).await {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("Bridge: no se pudo vincular al puerto {}: {}", BRIDGE_PORT, e);
                    running.store(false, Ordering::SeqCst);
                    let _ = app_handle.emit("bridge-status", false);
                    return;
                }
            };

            axum::serve(listener, routes)
                .with_graceful_shutdown(async move {
                    loop {
                        if shutdown_rx.changed().await.is_err() {
                            break;
                        }
                        if *shutdown_rx.borrow() {
                            break;
                        }
                    }
                })
                .await
                .ok();

            running.store(false, Ordering::SeqCst);
            let _ = app_handle.emit("bridge-status", false);
        });
    });
}

#[tauri::command]
fn bridge_status(state: State<'_, BridgeState>) -> Result<bool, String> {
    Ok(state.running.load(Ordering::SeqCst))
}

#[tauri::command]
fn restart_bridge(
    app_handle: tauri::AppHandle,
    state: State<'_, BridgeState>,
) -> Result<String, String> {
    if let Some(tx) = state.shutdown_tx.lock().unwrap().take() {
        let _ = tx.send(true);
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    spawn_bridge(
        app_handle.clone(),
        state.running.clone(),
        state.shutdown_tx.clone(),
    );

    Ok(format!("Bridge reiniciado en el puerto {}", BRIDGE_PORT))
}

// =========================================================================
// 12. ENTRADA PRINCIPAL DE TAURI v2
// =========================================================================

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let bridge_running = Arc::new(AtomicBool::new(false));
    let bridge_shutdown_tx: Arc<Mutex<Option<tokio::sync::watch::Sender<bool>>>> = Arc::new(Mutex::new(None));

    tauri::Builder::default()
        .manage(AppConfigState {
            config: Mutex::new(None),
        })
        .manage(BridgeState {
            running: bridge_running.clone(),
            shutdown_tx: bridge_shutdown_tx.clone(),
        })
        .manage(PrintHistory {
            logs: Arc::new(Mutex::new(Vec::new())),
        })
        .setup(move |app| {
            // Cargar configuración guardada en disco (printer_config.json)
            let loaded_config = load_config_from_disk(app.handle());

            // Inicializar el estado de la aplicación con la configuración cargada
            *app.state::<AppConfigState>().config.lock().unwrap() = Some(loaded_config);

            // Iniciar el bridge automáticamente al arrancar la app
            spawn_bridge(
                app.handle().clone(),
                bridge_running,
                bridge_shutdown_tx,
            );
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_printers,
            get_config,
            save_config,
            print_test_page,
            print_test_ticket,
            bridge_status,
            restart_bridge,
            get_print_history
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}