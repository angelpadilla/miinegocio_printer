# 🧾 Miinegocio Printer — Puente de Impresión POS

Aplicación de escritorio (Tauri + Vue + Rust) que actúa como **puente local** entre tu app Rails 8 (o cualquier frontend web) y tu impresora térmica USB. Expone un endpoint HTTP local en el puerto `9876` para recibir tickets y los imprime directamente.

---

## 🚀 Características Principales

* 💾 **Persistencia Automática (JSON)**: Guarda la impresora seleccionada y el tamaño de papel en `printer_config.json` para que todo esté listo al abrir la app.
* 🪟🐧 **Multiplataforma (Linux + Windows)**: Funciona en Linux y Windows (soporte nativo para `usbprint.sys` en Windows sin necesidad de instalar controladores Zadig).

---

## 🚀 Cómo funciona el Bridge

```
┌──────────────┐     POST /print (JSON)      ┌──────────────────┐     USB      ┌──────────────┐
│   Rails 8    │ ──────────────────────────▶ │  Miinegocio      │ ──────────▶ │  Impresora   │
│  (navegador) │    http://127.0.0.1:9876    │  Printer (Tauri) │             │  Térmica     │
└──────────────┘                              └──────────────────┘             └──────────────┘
```

1. Abre la app de escritorio **Miinegocio Printer**
2. Selecciona tu impresora USB y tamaño de papel
3. Haz clic en **💾 Guardar Configuración**
4. El servidor HTTP interno arranca en `http://127.0.0.1:9876`
5. Desde tu app Rails 8 (o cliente HTTP), envías un `POST` con el ticket en JSON
6. La impresora imprime al instante

---

## 📮 Endpoint del Bridge

| Campo         | Valor                          |
|---------------|--------------------------------|
| **URL**       | `http://127.0.0.1:9876/print`  |
| **Método**    | `POST`                         |
| **Content-Type** | `application/json`          |
| **CORS**      | Permitido desde cualquier origen |

---

## 📦 Estructura del JSON (Formato Universal)

### Body de la petición

```json
{
  "store_name": "Mi Negocio S.A. de C.V.",
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
}
```

### Campos

| Campo | Tipo | Obligatorio | Descripción |
|---|---|---|---|
| `store_name` | string | ✅ | Nombre del negocio (encabezado del ticket) |
| `logo_url` | string | ❌ | URL del logotipo para imprimir (PNG recomendado) |
| `text_lines_before_items` | array | ❌ | Líneas de texto antes de los artículos (RFC, dirección, etc.) |
| `items` | array | ✅ | Lista de productos vendidos |
| `items[].name` | string | ✅ | Nombre del producto |
| `items[].price` | number | ✅ | Precio unitario (decimal) |
| `items[].qty` | number | ✅ | Cantidad vendida |
| `subtotal` | number | ❌ | Subtotal de la venta |
| `iva` | number | ❌ | IVA / impuesto |
| `total` | number | ✅ | Total de la venta |
| `text_lines_after_items` | array | ❌ | Líneas después de total/subtotal/IVA. Para información adicional, notas o leyendas |
| `barcode` | object | ❌ | Código de barras o QR (opcional, después de text_lines_after_items) |
| `barcode.type` | string | ❌ | `"bar"` o `"qr"` |
| `barcode.value` | string | ❌ | Valor del código |

### Tabla de artículos en el ticket impreso

El ticket muestra los artículos en una tabla de 4 columnas:

| Columna | Descripción |
|---|---|
| **Item** | Nombre del producto (alineado a la izquierda) |
| **Precio** | Precio unitario (alineado a la derecha) |
| **Cant** | Cantidad vendida (alineado a la derecha) |
| **Monto** | Total por línea = precio × cantidad (alineado a la derecha, calculado automáticamente) |

### TextLine (`text_lines_before_items` / `text_lines_after_items`)

| Campo | Tipo | Default | Descripción |
|---|---|---|---|
| `label` | string | — | Texto de la etiqueta |
| `label_bold` | bool | `false` | Negrita para la etiqueta |
| `value` | string | — | Valor o texto |
| `value_bold` | bool | `false` | Negrita para el valor |
| `font_size` | number | `12` | Tamaño de fuente (12 = normal) |
| `alignment` | string | `"left"` | `"left"`, `"right"`, `"center"`, `"space_between"` |

### Respuestas

| Código | Significado |
|---|---|
| `200 OK` | Ticket impreso correctamente |
| `400 Bad Request` | No hay impresora configurada (no se ha activado el bridge) |
| `500 Internal Server Error` | Error de hardware (impresora desconectada, sin papel, etc.) |

---

## 💎 Ejemplos de uso

### Ruby on Rails con `Faraday`

```ruby
# Gemfile
gem "faraday"

# config/initializers/printer_bridge.rb
module PrinterBridge
  URL = "http://127.0.0.1:9876/print"

  def self.print_ticket(store_name:, items:, total:, subtotal: nil, iva: nil,
                         text_lines_before_items: [], text_lines_after_items: [],
                         logo_url: nil, barcode: nil)
    conn = Faraday.new(url: URL) do |f|
      f.request :json
      f.adapter Faraday.default_adapter
    end

    payload = { store_name: store_name, items: items, total: total }
    payload[:logo_url] = logo_url if logo_url
    payload[:text_lines_before_items] = text_lines_before_items if text_lines_before_items.any?
    payload[:subtotal] = subtotal if subtotal
    payload[:iva] = iva if iva
    payload[:text_lines_after_items] = text_lines_after_items if text_lines_after_items.any?
    payload[:barcode] = barcode if barcode

    response = conn.post { |req| req.body = payload }
    { success: response.status == 200, message: response.body }
  rescue Faraday::ConnectionFailed
    { success: false, message: "El puente no está activo." }
  end
end

# Uso desde un controlador Rails
class VentasController < ApplicationController
  def create
    @venta = Venta.new(venta_params)
    if @venta.save
      PrinterBridge.print_ticket(
        store_name: current_user.negocio.nombre,
        logo_url: current_user.negocio.logo_url,
        text_lines_before_items: [
          { label: "RFC: ", label_bold: true, value: current_user.negocio.rfc,
            value_bold: true, alignment: "space_between" }
        ],
        items: @venta.productos.map { |p|
          { name: p.nombre, price: p.precio.to_f, qty: p.cantidad.to_i }
        },
        subtotal: @venta.subtotal.to_f, iva: @venta.iva.to_f,
        total: @venta.total.to_f,
        text_lines_after_items: [
          { label: "¡Gracias por su compra!", alignment: "center", font_size: 14 }
        ],
        barcode: { type: "qr", value: ticket_url(@venta) }
      )
      redirect_to @venta, notice: "Venta registrada. Ticket enviado a impresión."
    else
      render :new, status: :unprocessable_entity
    end
  end
end
```

### Con `Net::HTTP` (Ruby puro)

### Con `fetch` desde JavaScript

```javascript
// app/javascript/controllers/printer_controller.js
import { Controller } from "@hotwired/stimulus";

export default class extends Controller {
  async print() {
    const ticket = JSON.parse(this.element.dataset.ticket);

    try {
      const response = await fetch("http://127.0.0.1:9876/print", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(ticket),
      });

      if (response.ok) {
        alert("🧾 Ticket impreso");
      } else {
        alert("❌ " + (await response.text()));
      }
    } catch (err) {
      alert("⚠️ El puente de impresión no está activo. Abre Miinegocio Printer.");
    }
  }
}
```

```erb
<%# app/views/ventas/show.html.erb %>
<button data-controller="printer"
        data-action="click->printer#print"
        data-ticket="<%= {
          store_name: @venta.negocio.nombre,
          logo_url: @venta.negocio.logo_url,
          text_lines_before_items: [
            { label: 'RFC: ', label_bold: true, value: @venta.negocio.rfc,
              value_bold: true, alignment: 'space_between' }
          ],
          items: @venta.productos.map { |p|
            { name: p.nombre, price: p.precio.to_f, qty: p.cantidad.to_i }
          },
          subtotal: @venta.subtotal.to_f,
          iva: @venta.iva.to_f,
          total: @venta.total.to_f,
          text_lines_after_items: [
            { label: '¡Gracias por su compra!', alignment: 'center', font_size: 14 }
          ],
          barcode: { type: 'qr', value: ticket_url(@venta) }
        }.to_json %>">
  🧾 Imprimir Ticket
</button>
```

### Ruby puro con `Net::HTTP`

```ruby
require 'net/http'
require 'json'

def imprimir_ticket(venta)
  uri = URI("http://127.0.0.1:9876/print")

  payload = {
    store_name: venta.negocio.nombre,
    items: venta.productos.map do |p|
      { name: p.nombre, price: p.precio.to_f, qty: p.cantidad.to_i }
    end,
    subtotal: venta.subtotal.to_f,
    iva: venta.iva.to_f,
    total: venta.total.to_f
  }

  http = Net::HTTP.new(uri.host, uri.port)
  request = Net::HTTP::Post.new(uri.path, { "Content-Type" => "application/json" })
  request.body = payload.to_json

  response = http.request(request)

  if response.code == "200"
    puts "✅ Ticket impreso"
  else
    puts "❌ Error: #{response.body}"
  end
end
```

### Python

```python
import requests

def imprimir_ticket(venta):
    url = "http://127.0.0.1:9876/print"
    payload = {
        "store_name": venta["negocio"]["nombre"],
        "items": [
            {"name": p["nombre"], "price": float(p["precio"]), "qty": int(p["cantidad"])}
            for p in venta["productos"]
        ],
        "subtotal": float(venta["subtotal"]),
        "iva": float(venta["iva"]),
        "total": float(venta["total"]),
    }
    response = requests.post(url, json=payload)
    if response.status_code == 200:
        print("✅ Ticket impreso")
    else:
        print(f"❌ Error: {response.text}")
```

---

## 🖥️ Desarrollo local

```bash
# Instalar dependencias
npm install

# Ejecutar en modo desarrollo
npm run tauri dev

# Solo frontend (sin Rust)
npm run dev
```

---

## 📋 Requisitos

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://www.rust-lang.org/) 1.70+
- [Tauri CLI](https://v2.tauri.app/) (`npm install -g @tauri-apps/cli`)
- Linux: dependencias del sistema para Tauri
  ```bash
  sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev
  ```

---

## 🔧 Solución de problemas

| Problema | Solución |
|---|---|
| La app no detecta la impresora | Verifica que esté conectada por USB. Crea una regla udev si no tienes permisos (ver abajo) |
| `Error de hardware` al imprimir | La impresora puede estar desconectada o sin papel |
| Rails no puede conectar al bridge | Asegúrate de que la app de escritorio esté abierta y el bridge activado (botón verde) |
| La app no muestra nombres de dispositivos | Los nombres se leen de manufactura/producto USB. Si no existen, se muestra el ID |

### Regla udev para permisos USB (Linux)

```bash
# /etc/udev/rules.d/99-usb-printers.rules
SUBSYSTEM=="usb", ATTRS{idVendor}=="0456", ATTRS{idProduct}=="0808", MODE="0666"
```

```bash
sudo udevadm control --reload-rules && sudo udevadm trigger
```
