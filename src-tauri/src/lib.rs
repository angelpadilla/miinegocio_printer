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
use axum::{routing::post, Router, http::StatusCode, http::Method};
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
    /// Número de caracteres por línea para ocupar el 100% del ancho del papel:
    /// - 60mm / 58mm: 48 caracteres
    /// - 80mm: 64 caracteres
    pub fn chars_per_line(&self) -> usize {
        match self {
            PaperSize::Size60mm => 48,
            PaperSize::Size80mm => 64,
        }
    }

    pub fn separator(&self) -> String {
        "-".repeat(self.chars_per_line())
    }

    /// Formatea dos columnas alineadas a los extremos (Izquierda y Derecha)
    /// Calculando la longitud exacta de caracteres Unicode.
    pub fn format_two_cols(&self, left: &str, right: &str) -> String {
        self.format_two_cols_sized(left, right, 1)
    }

    /// Formatea dos columnas considerando el factor de escala de fuente
    /// (ej. tamaño 2x2 reduce el número de caracteres disponibles a la mitad).
    pub fn format_two_cols_sized(&self, left: &str, right: &str, size_mult: usize) -> String {
        let mult = size_mult.max(1);
        let total_width = (self.chars_per_line() / mult).max(1);
        let left_len = left.chars().count();
        let right_len = right.chars().count();

        if left_len + right_len >= total_width {
            format!("{} {}", left, right)
        } else {
            let spaces = total_width.saturating_sub(left_len + right_len);
            format!("{}{}{}", left, " ".repeat(spaces), right)
        }
    }

    /// Formatea fila de 4 columnas: Item, Precio, Cant, Monto ocupando todo el ancho
    pub fn format_table_row(&self, item: &str, price: &str, qty: &str, amount: &str) -> String {
        let total_width = self.chars_per_line();
        let (price_w, qty_w, amount_w) = match self {
            PaperSize::Size60mm => (8, 6, 8),
            PaperSize::Size80mm => (10, 8, 10),
        };
        let item_w = total_width.saturating_sub(price_w + qty_w + amount_w);

        let item_display: String = if item.chars().count() > item_w {
            item.chars().take(item_w.saturating_sub(1)).collect::<String>() + "…"
        } else {
            let spaces = item_w.saturating_sub(item.chars().count());
            format!("{}{}", item, " ".repeat(spaces))
        };

        format!(
            "{}{:>price_w$}{:>qty_w$}{:>amount_w$}",
            item_display,
            price,
            qty,
            amount,
            price_w = price_w,
            qty_w = qty_w,
            amount_w = amount_w,
        )
    }

    pub fn format_table_header(&self) -> String {
        self.format_table_row("Item", "Precio", "Cant", "Monto")
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UsbPrinter {
    pub vendor_id: u16,
    pub product_id: u16,
    pub device_path: Option<String>,
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PrinterConfig {
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
    pub device_path: Option<String>,
    pub paper_size: PaperSize,
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

use serde::de::{self, Deserializer};

fn deserialize_flexible_f64<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum FlexF64 {
        Num(f64),
        Int(i64),
        Str(String),
    }

    match FlexF64::deserialize(deserializer)? {
        FlexF64::Num(n) => Ok(n),
        FlexF64::Int(i) => Ok(i as f64),
        FlexF64::Str(s) => {
            let clean = s.trim().trim_start_matches('$').replace(',', "");
            clean.parse::<f64>().map_err(de::Error::custom)
        }
    }
}

fn deserialize_optional_flexible_f64<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum FlexOptF64 {
        Num(f64),
        Int(i64),
        Str(String),
    }

    match Option::<FlexOptF64>::deserialize(deserializer)? {
        Some(FlexOptF64::Num(n)) => Ok(Some(n)),
        Some(FlexOptF64::Int(i)) => Ok(Some(i as f64)),
        Some(FlexOptF64::Str(s)) => {
            let clean = s.trim().trim_start_matches('$').replace(',', "");
            if clean.is_empty() {
                Ok(None)
            } else {
                clean.parse::<f64>().map(Some).map_err(de::Error::custom)
            }
        }
        None => Ok(None),
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TicketPayload {
    /// Nombre de la tienda (se muestra como encabezado principal)
    #[serde(alias = "store_name", alias = "storeName", alias = "store")]
    pub store_name: String,

    /// URL opcional del logotipo para imprimir
    #[serde(default, alias = "logo_url", alias = "logoUrl", alias = "logo")]
    pub logo_url: Option<String>,

    /// Líneas de texto antes de los artículos
    #[serde(default, alias = "text_lines_before_items", alias = "textLinesBeforeItems", alias = "text_lines_before", alias = "lines_before")]
    pub text_lines_before_items: Vec<TextLine>,

    /// Lista de artículos/items
    #[serde(alias = "items", alias = "products", alias = "articles")]
    pub items: Vec<TicketItem>,

    /// Subtotal (opcional)
    #[serde(default, alias = "subtotal", alias = "subTotal", alias = "sub_total", deserialize_with = "deserialize_optional_flexible_f64")]
    pub subtotal: Option<f64>,

    /// IVA / Impuesto (opcional)
    #[serde(default, alias = "iva", alias = "IVA", alias = "tax", alias = "taxes", deserialize_with = "deserialize_optional_flexible_f64")]
    pub iva: Option<f64>,

    /// Total de la venta
    #[serde(alias = "total", alias = "total_amount", alias = "totalAmount", deserialize_with = "deserialize_flexible_f64")]
    pub total: f64,

    /// Líneas de texto después de totales (subtotal/iva/total).
    #[serde(default, alias = "text_lines_after_items", alias = "textLinesAfterItems", alias = "text_lines_after", alias = "lines_after")]
    pub text_lines_after_items: Vec<TextLine>,

    /// Código de barras o QR opcional
    #[serde(default, alias = "barcode", alias = "barCode", alias = "qr", alias = "qrcode")]
    pub barcode: Option<BarcodeInfo>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TicketItem {
    #[serde(alias = "name", alias = "title", alias = "description")]
    pub name: String,
    #[serde(alias = "price", alias = "unit_price", alias = "unitPrice", deserialize_with = "deserialize_flexible_f64")]
    pub price: f64,
    #[serde(alias = "qty", alias = "quantity", alias = "count", deserialize_with = "deserialize_flexible_f64")]
    pub qty: f64,
}

fn deserialize_flexible_u32<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum FlexU32 {
        Num(u32),
        Str(String),
    }

    match Option::<FlexU32>::deserialize(deserializer)? {
        Some(FlexU32::Num(n)) => Ok(n),
        Some(FlexU32::Str(s)) => s.trim().parse::<u32>().map_err(de::Error::custom),
        None => Ok(default_font_size()),
    }
}

fn deserialize_flexible_bool<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum FlexBool {
        Bool(bool),
        Num(i64),
        Str(String),
    }

    match Option::<FlexBool>::deserialize(deserializer)? {
        Some(FlexBool::Bool(b)) => Ok(b),
        Some(FlexBool::Num(n)) => Ok(n != 0),
        Some(FlexBool::Str(s)) => {
            let lower = s.trim().to_lowercase();
            Ok(lower == "true" || lower == "1" || lower == "yes" || lower == "si")
        }
        None => Ok(false),
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TextLine {
    /// Texto de la etiqueta (opcional)
    #[serde(default, alias = "label", alias = "title", alias = "key")]
    pub label: Option<String>,
    /// Si la etiqueta debe ir en negrita
    #[serde(default, alias = "label_bold", alias = "labelBold", alias = "bold", deserialize_with = "deserialize_flexible_bool")]
    pub label_bold: bool,
    /// Valor/texto (opcional)
    #[serde(default, alias = "value", alias = "text", alias = "val")]
    pub value: Option<String>,
    /// Si el valor debe ir en negrita
    #[serde(default, alias = "value_bold", alias = "valueBold", deserialize_with = "deserialize_flexible_bool")]
    pub value_bold: bool,
    /// Tamaño de fuente (12 = normal)
    #[serde(default = "default_font_size", alias = "font_size", alias = "fontSize", alias = "size", deserialize_with = "deserialize_flexible_u32")]
    pub font_size: u32,
    /// Alineación del texto
    #[serde(default, alias = "alignment", alias = "align")]
    pub alignment: TextAlignment,
}

fn default_font_size() -> u32 {
    12
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub enum TextAlignment {
    #[default]
    #[serde(rename = "left", alias = "Left", alias = "LEFT")]
    Left,
    #[serde(rename = "right", alias = "Right", alias = "RIGHT")]
    Right,
    #[serde(rename = "center", alias = "Center", alias = "CENTER")]
    Center,
    #[serde(rename = "space_between", alias = "spaceBetween", alias = "space_between", alias = "SpaceBetween", alias = "space-between")]
    SpaceBetween,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BarcodeInfo {
    /// Tipo: "bar" para código de barras, "qr" para código QR
    #[serde(rename = "type", alias = "barcode_type", alias = "barcodeType")]
    pub barcode_type: BarcodeType,
    /// Valor/datos del código
    pub value: String,
    /// Alineación del código: "left", "center", "right" (por defecto "center")
    #[serde(default = "default_barcode_alignment", alias = "alignment", alias = "align")]
    pub alignment: TextAlignment,
}

fn default_barcode_alignment() -> TextAlignment {
    TextAlignment::Center
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum BarcodeType {
    #[serde(rename = "bar", alias = "Bar", alias = "BAR", alias = "barcode", alias = "code39")]
    Bar,
    #[serde(rename = "qr", alias = "Qr", alias = "QR", alias = "qrcode")]
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
    pub timestamp: String,
    pub source: String,       // "Bridge HTTP" o "Prueba local"
    pub store_name: String,
    pub total: String,
    pub item_count: usize,
    pub success: bool,
    pub error_message: Option<String>,
    pub raw_json: Option<String>,
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
        if let Some(ref path) = config.device_path {
            if let Ok(driver) = WindowsUsbPrintDriver::open(path) {
                return Ok(DynamicDriver(Box::new(driver)));
            }
        }

        if let (Some(vid), Some(pid)) = (config.vendor_id, config.product_id) {
            if let Ok(driver) = WindowsUsbPrintDriver::open_by_vid_pid(vid, pid) {
                return Ok(DynamicDriver(Box::new(driver)));
            }

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

    save_config_to_disk(&app_handle, &config)?;

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
        .page_code(PageCode::PC858).map_err(|e| e.to_string())?
        .justify(JustifyMode::CENTER).map_err(|e| e.to_string())?
        .size(2, 2).map_err(|e| e.to_string())?
        .bold(true).map_err(|e| e.to_string())?
        .writeln("HOJA DE PRUEBA").map_err(|e| e.to_string())?
        .bold(false).map_err(|e| e.to_string())?
        .reset_size().map_err(|e| e.to_string())?
        .writeln("Miinegocio Printer").map_err(|e| e.to_string())?
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
        error_message: result.as_ref().err().cloned(),
        raw_json: None,
    };

    push_log_and_emit(&app_handle, log);

    result.map(|_| ()).map_err(|e| e.to_string())
}

/// Imprime un ticket de prueba completo con el formato universal
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
        store_name: "Hoja de prueba".to_string(),
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
                alignment: TextAlignment::SpaceBetween,
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
            alignment: TextAlignment::Center,
        }),
    };

    let raw_json = serde_json::to_string_pretty(&payload).ok();
    let result = execute_print(config, payload).await;

    let log = PrintLogEntry {
        timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        source: "🧪 Prueba completa".into(),
        store_name: "Hoja de prueba".into(),
        total: "$168.78".into(),
        item_count: 3,
        success: result.is_ok(),
        error_message: result.as_ref().err().cloned(),
        raw_json,
    };

    push_log_and_emit(&app_handle, log);

    result
}

// =========================================================================
// 6. HELPER: IMPRESIÓN DE LOGO Y QR
// =========================================================================

async fn download_logo_bytes(url: &str) -> Option<Vec<u8>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .ok()?;
    let response = client.get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let bytes = response.bytes().await.ok()?.to_vec();
    // Validar que sea una imagen decodificable
    if image::load_from_memory(&bytes).is_ok() {
        Some(bytes)
    } else {
        None
    }
}

/// Genera una imagen PNG monocromática del código QR en memoria
/// con el lienzo ajustado al ancho del cabezal térmico (576 px para 80mm / 384 px para 60mm)
/// para centrar o alinear horizontalmente el código QR con precisión en todas las impresoras.
fn generate_qr_png_bytes(data: &str, p: &PaperSize, alignment: &TextAlignment) -> Option<Vec<u8>> {
    use qrcode::{QrCode, Color};

    let code = QrCode::new(data.as_bytes()).ok()?;
    let width = code.width();
    let scale = match p {
        PaperSize::Size60mm => 4, // 4 pixels por módulo
        PaperSize::Size80mm => 6, // 6 pixels por módulo
    };
    let border = 2; // zona de silencio
    let qr_pixels = ((width + border * 2) * scale) as u32;

    let total_paper_width = match p {
        PaperSize::Size60mm => 384u32, // 58mm / 60mm = 384 puntos
        PaperSize::Size80mm => 576u32, // 80mm = 576 puntos
    };

    let canvas_width = total_paper_width.max(qr_pixels);

    let left_offset = match alignment {
        TextAlignment::Left => 0u32,
        TextAlignment::Right => canvas_width.saturating_sub(qr_pixels),
        _ => canvas_width.saturating_sub(qr_pixels) / 2, // Centrado
    };

    let mut img = image::GrayImage::new(canvas_width, qr_pixels);
    // Fondo blanco
    for pixel in img.pixels_mut() {
        *pixel = image::Luma([255u8]);
    }

    // Módulos negros del código QR
    for y in 0..width {
        for x in 0..width {
            if code[(x, y)] == Color::Dark {
                let px_start = left_offset + ((x + border) * scale) as u32;
                let py_start = ((y + border) * scale) as u32;
                for dy in 0..scale as u32 {
                    for dx in 0..scale as u32 {
                        if px_start + dx < canvas_width {
                            img.put_pixel(px_start + dx, py_start + dy, image::Luma([0u8]));
                        }
                    }
                }
            }
        }
    }

    let mut png_bytes = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut png_bytes);
    img.write_to(&mut cursor, image::ImageOutputFormat::Png).ok()?;
    Some(png_bytes)
}

// =========================================================================
// 7. HELPER: IMPRESIÓN DE UNA LÍNEA DE TEXTO (TextLine)
// =========================================================================

fn print_text_line(
    printer: &mut Printer<DynamicDriver>,
    p: &PaperSize,
    line: &TextLine,
) -> Result<(), String> {
    let (font_type, width_mult, height_mult) = if line.font_size < 12 {
        (Font::B, 1u8, 1u8) // Fuente compacta/pequeña (9x17)
    } else if line.font_size < 14 {
        (Font::A, 1u8, 1u8) // Fuente estándar normal (12x24)
    } else if line.font_size < 18 {
        (Font::A, 1u8, 2u8) // Doble Altura (altura 2x, ancho normal 1x)
    } else if line.font_size < 24 {
        (Font::A, 2u8, 2u8) // Doble Ancho y Doble Alto (2x2)
    } else {
        (Font::A, 3u8, 3u8) // Grande (3x3)
    };

    if font_type != Font::A {
        printer.font(font_type).map_err(|e| e.to_string())?;
    }

    if width_mult > 1 || height_mult > 1 {
        printer.size(width_mult, height_mult).map_err(|e| e.to_string())?;
    }

    match line.alignment {
        TextAlignment::SpaceBetween => {
            let left = line.label.as_deref().unwrap_or("");
            let right = line.value.as_deref().unwrap_or("");
            let mult = (width_mult as usize).max(1);
            let base_chars = match font_type {
                Font::B => p.chars_per_line() * 4 / 3,
                _ => p.chars_per_line(),
            };
            let total_width = (base_chars / mult).max(1);
            let left_len = left.chars().count();
            let right_len = right.chars().count();
            let spaces_count = total_width.saturating_sub(left_len + right_len);
            let spaces = " ".repeat(spaces_count);

            printer.justify(JustifyMode::LEFT).map_err(|e| e.to_string())?;

            if !left.is_empty() {
                if line.label_bold {
                    printer.bold(true).map_err(|e| e.to_string())?;
                }
                printer.write(left).map_err(|e| e.to_string())?;
                if line.label_bold {
                    printer.bold(false).map_err(|e| e.to_string())?;
                }
            }

            if !spaces.is_empty() {
                printer.write(&spaces).map_err(|e| e.to_string())?;
            }

            if !right.is_empty() {
                if line.value_bold {
                    printer.bold(true).map_err(|e| e.to_string())?;
                }
                printer.write(right).map_err(|e| e.to_string())?;
                if line.value_bold {
                    printer.bold(false).map_err(|e| e.to_string())?;
                }
            }

            printer.feed().map_err(|e| e.to_string())?;
        }
        _ => {
            let justify = match line.alignment {
                TextAlignment::Center => JustifyMode::CENTER,
                TextAlignment::Right => JustifyMode::RIGHT,
                _ => JustifyMode::LEFT,
            };

            printer.justify(justify).map_err(|e| e.to_string())?;

            if let Some(l) = &line.label {
                if line.label_bold {
                    printer.bold(true).map_err(|e| e.to_string())?;
                }
                printer.write(l).map_err(|e| e.to_string())?;
                if line.label_bold {
                    printer.bold(false).map_err(|e| e.to_string())?;
                }
            }

            if let Some(v) = &line.value {
                if line.value_bold {
                    printer.bold(true).map_err(|e| e.to_string())?;
                }
                printer.write(v).map_err(|e| e.to_string())?;
                if line.value_bold {
                    printer.bold(false).map_err(|e| e.to_string())?;
                }
            }

            printer.feed().map_err(|e| e.to_string())?;
        }
    }

    if width_mult > 1 || height_mult > 1 {
        printer.reset_size().map_err(|e| e.to_string())?;
    }
    if font_type != Font::A {
        printer.font(Font::A).map_err(|e| e.to_string())?;
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

    printer
        .init().map_err(|e| e.to_string())?
        .page_code(PageCode::PC858).map_err(|e| e.to_string())?;

    // ─── LOGO (opcional, solo si es una imagen válida) ───────────────────
    if let Some(logo_url) = &payload.logo_url {
        if let Some(bytes) = download_logo_bytes(logo_url).await {
            let _ = printer
                .justify(JustifyMode::CENTER)
                .map_err(|e| e.to_string())?
                .bit_image_from_bytes(&bytes)
                .map_err(|e| e.to_string());
            let _ = printer.feed();
        }
    }

    // ─── ENCABEZADO ──────────────────────────────────────────────────────
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

    // ─── LÍNEAS ANTES DE ARTÍCULOS ───────────────────────────────────────
    if !payload.text_lines_before_items.is_empty() {
        printer.feed().map_err(|e| e.to_string())?;
        for line in &payload.text_lines_before_items {
            print_text_line(&mut printer, p, line)?;
        }
    }

    // ─── TABLA DE ARTÍCULOS ──────────────────────────────────────────────
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

    // ─── SUBTOTAL E IVA (opcionales) ─────────────────────────────────────
    if let Some(subtotal) = payload.subtotal {
        printer
            .justify(JustifyMode::LEFT)
            .map_err(|e| e.to_string())?
            .writeln(&p.format_two_cols("Subtotal:", &format!("${:.2}", subtotal)))
            .map_err(|e| e.to_string())?;
    }
    if let Some(iva) = payload.iva {
        printer
            .justify(JustifyMode::LEFT)
            .map_err(|e| e.to_string())?
            .writeln(&p.format_two_cols("IVA:", &format!("${:.2}", iva)))
            .map_err(|e| e.to_string())?;
    }

    // ─── TOTAL ───────────────────────────────────────────────────────────
    let total_str = format!("${:.2}", payload.total);
    printer
        .justify(JustifyMode::LEFT)
        .map_err(|e| e.to_string())?
        .size(2, 2)
        .map_err(|e| e.to_string())?
        .bold(true)
        .map_err(|e| e.to_string())?
        .writeln(&p.format_two_cols_sized("TOTAL:", &total_str, 2))
        .map_err(|e| e.to_string())?
        .bold(false)
        .map_err(|e| e.to_string())?
        .reset_size()
        .map_err(|e| e.to_string())?
        .feed()
        .map_err(|e| e.to_string())?;

    // ─── LÍNEAS DESPUÉS DE TOTALES ───────────────────────────────────────
    for line in &payload.text_lines_after_items {
        print_text_line(&mut printer, p, line)?;
    }

    // ─── CÓDIGO DE BARRAS / QR (opcional) ────────────────────────────────
    if let Some(barcode) = &payload.barcode {
        printer.feed().map_err(|e| e.to_string())?;

        let justify = match barcode.alignment {
            TextAlignment::Left => JustifyMode::LEFT,
            TextAlignment::Right => JustifyMode::RIGHT,
            _ => JustifyMode::CENTER,
        };

        printer.justify(justify).map_err(|e| e.to_string())?;

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
                if let Some(png_bytes) = generate_qr_png_bytes(&barcode.value, p, &barcode.alignment) {
                    let _ = printer
                        .bit_image_from_bytes(&png_bytes)
                        .map_err(|e| e.to_string());
                } else {
                    let _ = printer.qrcode_option(
                        &barcode.value,
                        QRCodeOption::new(QRCodeModel::Model2, 4, QRCodeCorrectionLevel::M),
                    );
                }
            }
        }
    }

    // ─── PIE DE PÁGINA Y CORTE ──────────────────────────────────────────
    printer
        .feed()
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
    body_bytes: axum::body::Bytes,
) -> impl axum::response::IntoResponse {
    let body_str = match String::from_utf8(body_bytes.to_vec()) {
        Ok(s) => s,
        Err(_) => String::from_utf8_lossy(&body_bytes).to_string(),
    };

    let raw_json_formatted = if let Ok(val) = serde_json::from_str::<serde_json::Value>(&body_str) {
        serde_json::to_string_pretty(&val).unwrap_or_else(|_| body_str.clone())
    } else {
        body_str.clone()
    };

    let payload = match serde_json::from_str::<TicketPayload>(&body_str) {
        Ok(p) => p,
        Err(e) => {
            let error_msg = format!("Error al interpretar estructura JSON: {}", e);
            eprintln!("{}", error_msg);
            let log = PrintLogEntry {
                timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                source: "🌐 Bridge HTTP".into(),
                store_name: "Error en JSON".into(),
                total: "$ 0.00".into(),
                item_count: 0,
                success: false,
                error_message: Some(error_msg.clone()),
                raw_json: Some(raw_json_formatted),
            };
            push_log_and_emit(&handle, log);
            return (StatusCode::BAD_REQUEST, error_msg);
        }
    };

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
    let error_message = if success { None } else { Some(msg.clone()) };

    let log = PrintLogEntry {
        timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        source: "🌐 Bridge HTTP".into(),
        store_name: store,
        total: total_str,
        item_count: count,
        success,
        error_message,
        raw_json: Some(raw_json_formatted),
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
            let loaded_config = load_config_from_disk(app.handle());
            *app.state::<AppConfigState>().config.lock().unwrap() = Some(loaded_config);

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

#[cfg(test)]
mod tests {
    use super::*;
    use escpos::driver::ConsoleDriver;

    #[tokio::test]
    async fn test_full_print_simulation() {
        let json_data = r#"{
            "store_name": "Hoja de prueba",
            "logo_url": "https://ejemplo.com/logo.png",
            "text_lines_before_items": [
              {
                "label": "RFC: ",
                "label_bold": true,
                "value": "XAXX010101000",
                "value_bold": true,
                "font_size": 12,
                "alignment": "space_between"
              },
              {
                "label": "Dirección: ",
                "value": "Av. Principal #123, Centro",
                "font_size": 10,
                "alignment": "left"
              }
            ],
            "items": [
              {
                "name": "Café Americano",
                "price": 45.00,
                "qty": 2
              },
              {
                "name": "Croissant",
                "price": 30.00,
                "qty": 1
              }
            ],
            "subtotal": 75.00,
            "iva": 12.00,
            "total": 87.00,
            "text_lines_after_items": [
              {
                "label": "Forma de pago: ",
                "value": "Tarjeta de crédito",
                "font_size": 10,
                "alignment": "left"
              },
              {
              "label": "¡Gracias por su compra!",
              "font_size": 14,
              "alignment": "center"
              }
            ],
            "barcode": {
              "type": "qr",
              "value": "https://ejemplo.com/ticket/12345"
            }
        }"#;

        let payload: TicketPayload = serde_json::from_str(json_data).unwrap();
        let config = PrinterConfig {
            vendor_id: Some(0x0456),
            product_id: Some(0x0808),
            device_path: None,
            paper_size: PaperSize::Size80mm,
        };

        let driver = DynamicDriver(Box::new(ConsoleDriver::open(true)));
        let mut printer = Printer::new(driver, Default::default(), None);
        let p = &config.paper_size;

        println!("--- START PRINT SIMULATION ---");
        printer.init().unwrap();

        // Logo
        if let Some(logo_url) = &payload.logo_url {
            if let Some(bytes) = download_logo_bytes(logo_url).await {
                let _ = printer.justify(JustifyMode::CENTER).unwrap().bit_image_from_bytes(&bytes);
            }
        }

        // Encabezado
        printer
            .justify(JustifyMode::CENTER).unwrap()
            .size(2, 2).unwrap()
            .bold(true).unwrap()
            .writeln(&payload.store_name).unwrap()
            .bold(false).unwrap()
            .reset_size().unwrap();

        // Líneas antes
        if !payload.text_lines_before_items.is_empty() {
            printer.feed().unwrap();
            for line in &payload.text_lines_before_items {
                print_text_line(&mut printer, p, line).unwrap();
            }
        }

        // Tabla
        printer.feed().unwrap();
        printer
            .justify(JustifyMode::LEFT).unwrap()
            .writeln(&p.separator()).unwrap()
            .writeln(&p.format_table_header()).unwrap()
            .writeln(&p.separator()).unwrap();

        for item in &payload.items {
            let price_str = format!("${:.2}", item.price);
            let qty_str = format!("{:.2}", item.qty);
            let amount_str = format!("${:.2}", item.price * item.qty);
            printer
                .writeln(&p.format_table_row(&item.name, &price_str, &qty_str, &amount_str)).unwrap();
        }

        printer.writeln(&p.separator()).unwrap().feed().unwrap();

        // Subtotal e iva
        if let Some(subtotal) = payload.subtotal {
            printer
                .justify(JustifyMode::LEFT).unwrap()
                .writeln(&p.format_two_cols("Subtotal:", &format!("${:.2}", subtotal))).unwrap();
        }
        if let Some(iva) = payload.iva {
            printer
                .justify(JustifyMode::LEFT).unwrap()
                .writeln(&p.format_two_cols("IVA:", &format!("${:.2}", iva))).unwrap();
        }

        // Total
        let total_str = format!("${:.2}", payload.total);
        printer
            .justify(JustifyMode::LEFT).unwrap()
            .size(2, 2).unwrap()
            .bold(true).unwrap()
            .writeln(&p.format_two_cols_sized("TOTAL:", &total_str, 2)).unwrap()
            .bold(false).unwrap()
            .reset_size().unwrap()
            .feed().unwrap();

        // Líneas después
        for line in &payload.text_lines_after_items {
            print_text_line(&mut printer, p, line).unwrap();
        }

        // Barcode
        if let Some(barcode) = &payload.barcode {
            printer.feed().unwrap();
            if let Some(png_bytes) = generate_qr_png_bytes(&barcode.value, p, &barcode.alignment) {
                let _ = printer
                    .bit_image_from_bytes(&png_bytes);
            }
        }

        printer
            .feed().unwrap()
            .feed().unwrap()
            .print_cut().unwrap();
        println!("--- END PRINT SIMULATION ---");
    }

    #[test]
    fn test_camel_case_and_strings_deserialization() {
        let json_data = r#"{
            "storeName": "Tienda CamelCase",
            "logoUrl": "https://ejemplo.com/logo.png",
            "textLinesBeforeItems": [
              {
                "label": "RFC: ",
                "labelBold": true,
                "value": "XAXX010101000",
                "valueBold": true,
                "fontSize": "12",
                "alignment": "spaceBetween"
              }
            ],
            "items": [
              {
                "name": "Café",
                "price": "45.50",
                "qty": "2"
              }
            ],
            "subTotal": "91.00",
            "tax": "14.56",
            "total": "$105.56",
            "textLinesAfterItems": [
              {
                "label": "¡Gracias!",
                "alignment": "center"
              }
            ],
            "barcode": {
              "type": "qr",
              "value": "https://test.com"
            }
        }"#;

        let payload: TicketPayload = serde_json::from_str(json_data).unwrap();
        assert_eq!(payload.store_name, "Tienda CamelCase");
        assert_eq!(payload.text_lines_before_items.len(), 1);
        assert_eq!(payload.items.len(), 1);
        assert_eq!(payload.items[0].price, 45.50);
        assert_eq!(payload.items[0].qty, 2.0);
        assert_eq!(payload.subtotal, Some(91.00));
        assert_eq!(payload.iva, Some(14.56));
        assert_eq!(payload.total, 105.56);
        assert_eq!(payload.text_lines_after_items.len(), 1);
        assert!(payload.barcode.is_some());
    }
}