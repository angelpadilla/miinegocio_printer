use escpos::driver::NativeUsbDriver;
use escpos::printer::Printer;
use escpos::utils::*;
use escpos::errors::Result;
use std::env;

// Printer Device IDs
const VENDOR_ID: u16 = 0x0456; // Analog Devices, Inc.
const PRODUCT_ID: u16 = 0x0808; // USB POS Printer

// Paper Sizes for Thermal Printers
#[derive(Clone, Copy, Debug)]
enum PaperSize {
    Size60mm,  // 60mm (2.36 inches) - Standard thermal printer
    Size80mm,  // 80mm (3.15 inches) - Larger thermal printer
}

impl PaperSize {
    /// Get the number of characters that fit on a single line
    fn chars_per_line(&self) -> usize {
        match self {
            PaperSize::Size60mm => 48,  // 60mm thermal printer: ~48 chars at normal size
            PaperSize::Size80mm => 64,  // 80mm thermal printer: ~64 chars at normal size
        }
    }

    /// Get the width in mm
    fn width_mm(&self) -> u16 {
        match self {
            PaperSize::Size60mm => 60,
            PaperSize::Size80mm => 80,
        }
    }

    /// Get a separator line for formatting
    fn separator(&self) -> String {
        "=".repeat(self.chars_per_line())
    }

    /// Format a two-column line (label and value)
    fn format_two_cols(&self, left: &str, right: &str) -> String {
        let total_width = self.chars_per_line();
        let left_width = (total_width - right.len()).saturating_sub(1);
        format!("{:<width$} {}", left, right, width = left_width)
    }

    /// Format a table row with 4 columns (item, price, qty, amount)
    fn format_table_row(&self, item: &str, price: &str, qty: &str, amount: &str) -> String {
        let total_width = self.chars_per_line();
        let price_width = 10;  // Fixed width for price column
        let qty_width = 8;     // Fixed width for qty column
        let amount_width = 10; // Fixed width for amount column
        let item_width = total_width.saturating_sub(price_width + qty_width + amount_width);
        
        // Construct the row to fill exactly total_width characters
        format!("{:<item_w$}{:>price_w$}{:>qty_w$}{:>amount_w$}",
                item,
                price,
                qty,
                amount,
                item_w = item_width,
                price_w = price_width,
                qty_w = qty_width,
                amount_w = amount_width)
    }

    /// Format table header with 4 columns
    fn format_table_header(&self) -> String {
        self.format_table_row("Item", "Price", "Qty", "Amount")
    }
}

impl Default for PaperSize {
    fn default() -> Self {
        PaperSize::Size80mm  // Default to 80mm
    }
}

fn main() -> Result<()> {
    // Parse command line arguments
    let args: Vec<String> = env::args().collect();
    
    let paper_size = if args.len() >= 2 {
        match args[1].as_str() {
            "60" | "60mm" | "Size60mm" => PaperSize::Size60mm,
            "80" | "80mm" | "Size80mm" => PaperSize::Size80mm,
            "-h" | "--help" => {
                print_usage(&args[0]);
                return Ok(());
            }
            _ => {
                eprintln!("Error: Unknown paper size '{}'. Use '60' or '80'", args[1]);
                std::process::exit(1);
            }
        }
    } else {
        PaperSize::default()  // Default to 80mm
    };

    // Connect to USB POS Printer
    println!("Connecting to POS printer ({:04x}:{:04x})...", VENDOR_ID, PRODUCT_ID);
    println!("Paper size: {}mm", paper_size.width_mm());
    let driver = NativeUsbDriver::open(VENDOR_ID, PRODUCT_ID)?;
    println!("Connected!");

    // Initialize printer
    let mut printer = Printer::new(driver, Default::default(), None);

    // Start printing with paper size formatting
    printer
        .init()?
        // Header
        .justify(JustifyMode::CENTER)?
        .size(2, 2)?
        .bold(true)?
        .writeln("RECEIPT")?
        .bold(false)?
        .reset_size()?
        .writeln("Store: Example Store")?
        .writeln("Address: 123 Main St")?
        .feed()?
        
        // Logo/Image section
        .justify(JustifyMode::CENTER)?
        .bit_image_option(
            "logo.png",
            BitImageOption::new(Some(384), None, BitImageSize::Normal)?,
        )?
        .feed()?
        .justify(JustifyMode::LEFT)?
        .writeln(&paper_size.separator())?
        .writeln(&paper_size.format_table_header())?
        .writeln(&paper_size.separator())?
        .writeln(&paper_size.format_table_row("Coffee", "$3.50", "x2", "$7.00"))?
        .writeln(&paper_size.format_table_row("Croissant", "$4.25", "x1", "$4.25"))?
        .writeln(&paper_size.format_table_row("Orange Juice", "$2.50", "x1", "$2.50"))?
        .writeln(&paper_size.separator())?
        .feed()?
        
        // Totals
        .justify(JustifyMode::RIGHT)?
        .writeln(&paper_size.format_two_cols("Subtotal:", "$14.75"))?
        .writeln(&paper_size.format_two_cols("Tax (8%):", "$1.18"))?
        .size(2, 2)?
        .bold(true)?
        .writeln(&paper_size.format_two_cols("TOTAL:", "$15.93"))?
        .bold(false)?
        .reset_size()?
        .feed()?
        
        // Barcode section
        .justify(JustifyMode::CENTER)?
        .writeln("Order ID:")?
        .ean13_option(
            "1234567890265",
            BarcodeOption::new(
                BarcodeWidth::M,
                BarcodeHeight::M,
                BarcodeFont::A,
                BarcodePosition::Below,
            ),
        )?
        .feed()?
        
        // QR Code
        .writeln("Scan for receipt:")?
        .qrcode_option(
            "https://example.com/receipt/123456",
            QRCodeOption::new(QRCodeModel::Model2, 6, QRCodeCorrectionLevel::M),
        )?
        .feed()?
        .feed()?
        
        // Footer
        .writeln("Thank you for your purchase!")?
        .writeln("Visit us again!")?
        .feed()?
        .feed()?
        .feed()?
        
        // Cut paper
        .print_cut()?;

    println!("Ticket printed successfully!");
    Ok(())
}

fn print_usage(program_name: &str) {
    println!("POS Printer - USB Receipt Printer");
    println!();
    println!("Usage: {} [PAPER_SIZE]", program_name);
    println!();
    println!("Arguments:");
    println!("  PAPER_SIZE    Paper width in mm: 60 or 80 (default: 80)");
    println!();
    println!("Examples:");
    println!("  {}                    # Use default 80mm paper", program_name);
    println!("  {} 60                 # Use 60mm paper size", program_name);
    println!("  {} 80mm               # Use 80mm paper size (alternative)", program_name);
    println!("  {} --help             # Show this message", program_name);
    println!();
    println!("Paper Sizes:");
    println!("  60mm - Standard thermal printer (32 chars/line)");
    println!("  80mm - Larger thermal printer  (48 chars/line)");
}
