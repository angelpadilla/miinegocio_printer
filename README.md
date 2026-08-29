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
    "value": "https://ejemplo.com/ticket/12345",
    "alignment": "center"
  }
}
```

### Campos Principales

| Campo | Tipo | Obligatorio | Descripción |
|---|---|---|---|
| `store_name` | string | ✅ | Nombre del negocio (encabezado del ticket) |
| `logo_url` | string | ❌ | URL del logotipo para imprimir (PNG recomendado) |
| `text_lines_before_items` | array | ❌ | Líneas de texto antes de los artículos (RFC, dirección, etc.) |
| `items` | array | ✅ | Lista de productos vendidos |
| `items[].name` | string | ✅ | Nombre del producto |
| `items[].price` | number | ✅ | Precio unitario (decimal o string numérico) |
| `items[].qty` | number | ✅ | Cantidad vendida (decimal o entero) |
| `subtotal` | number | ❌ | Subtotal de la venta |
| `iva` | number | ❌ | IVA / impuesto |
| `total` | number | ✅ | Total de la venta |
| `text_lines_after_items` | array | ❌ | Líneas después de total/subtotal/IVA. Para notas, formas de pago o leyendas |
| `barcode` | object | ❌ | Código de barras o QR opcional (ver detalles abajo) |

### Código de Barras / QR (`barcode`)

| Campo | Tipo | Default | Descripción |
|---|---|---|---|
| `type` | string | ✅ | `"bar"` para código de barras (Code 39) o `"qr"` para código QR |
| `value` | string | ✅ | Valor, folio o URL del código |
| `alignment` | string | `"center"` | Alineación: `"left"`, `"center"`, `"right"` |

**Ejemplo Código de Barras:**
```json
"barcode": {
  "type": "bar",
  "value": "123123123",
  "alignment": "center"
}
```

**Ejemplo Código QR:**
```json
"barcode": {
  "type": "qr",
  "value": "https://ejemplo.com/ticket/12345",
  "alignment": "center"
}
```

### Tabla de artículos en el ticket impreso

El ticket muestra los artículos en una tabla de 4 columnas calculada automáticamente según el tamaño de papel:

| Columna | Alineación | Ancho en 80mm (64 car.) | Ancho en 60mm (48 car.) | Descripción |
|---|---|---|---|---|
| **Item** | Izquierda | Hasta 36 caracteres | Hasta 26 caracteres | Nombre del producto (se trunca con `…` si excede) |
| **Precio** | Derecha | 10 caracteres | 8 caracteres | Precio unitario formateado con `$X.XX` |
| **Cant** | Derecha | 8 caracteres | 6 caracteres | Cantidad vendida formateada con `X.XX` |
| **Monto** | Derecha | 10 caracteres | 8 caracteres | Total por línea = precio × cant formateado con `$X.XX` |

---

### 🔢 Capacidad de Dígitos y Decimales

#### Número de Decimales:
* **Montos monetarios (`price`, `subtotal`, `iva`, `total`, `monto de línea`)**: Se formatean y redondean siempre a **2 decimales** estándar de moneda (ej. `$45.00`, `$1,250.50`). En el JSON puedes enviar números con cualquier precisión flotante (`45.5`, `45.509`, `"45.50"`).
* **Cantidad (`qty`)**: Se formatea con **2 decimales** (ej. `2.00`, `1.50`, `0.75`), lo que permite tanto cantidades por piezas como ventas a granel o peso (kg/litros).

#### Máximo de Dígitos Enteros según el Papel:

| Campo | Papel de 80mm (64 car.) | Papel de 60mm / 58mm (48 car.) | Ejemplo de valor máximo |
|---|---|---|---|
| **Precio (`item.price`)** | Hasta **7 dígitos enteros** + 2 dec. | Hasta **5 dígitos enteros** + 2 dec. | `$9,999,999.99` (80mm) / `$99,999.99` (60mm) |
| **Cantidad (`item.qty`)** | Hasta **5 dígitos enteros** + 2 dec. | Hasta **3 dígitos enteros** + 2 dec. | `99,999.99` (80mm) / `999.99` (60mm) |
| **Monto (`price × qty`)** | Hasta **7 dígitos enteros** + 2 dec. | Hasta **5 dígitos enteros** + 2 dec. | `$9,999,999.99` (80mm) / `$99,999.99` (60mm) |
| **Subtotal e IVA** | Hasta **50+ dígitos enteros** | Hasta **35+ dígitos enteros** | Sin límite práctico |
| **TOTAL (fuente 2x2)** | Hasta **25 dígitos de monto** | Hasta **17 dígitos de monto** | `$999,999,999,999,999,999.99` |

---

### TextLine (`text_lines_before_items` / `text_lines_after_items`)

| Campo | Tipo | Default | Descripción |
|---|---|---|---|
| `label` | string | — | Texto de la etiqueta |
| `label_bold` | bool | `false` | Negrita solo para la etiqueta |
| `value` | string | — | Valor o texto |
| `value_bold` | bool | `false` | Negrita solo para el valor |
| `font_size` | number | `12` | Tamaño de fuente (ver tabla de escalas abajo) |
| `alignment` | string | `"left"` | `"left"`, `"right"`, `"center"`, `"space_between"` |

#### Escala de `font_size` en Impresoras Térmicas:

Las impresoras térmicas ESC/POS tienen fuentes de matriz de puntos fijas. La app traduce el número de `font_size` en las combinaciones óptimas de fuente y escala:

| `font_size` | Tipo de Fuente | Escala (Ancho × Alto) | Efecto Visual en el Ticket | Uso Recomendado |
|---|---|---|---|---|
| **`< 12`** (8, 9, 10, 11) | Font B (Compacta) | `1 × 1` | Letra pequeña y condensada | Leyendas, notas al pie, dirección, RFC |
| **`12 - 13`** (Default: 12) | Font A (Estándar) | `1 × 1` | Letra normal estándar | Texto general de lectura |
| **`14 - 17`** (14, 16) | Font A (Estándar) | `1 × 2` | **Doble alto (más estilizada sin ensanchar)** | Agradecimiento, folios destacados |
| **`18 - 23`** (18, 20) | Font A (Estándar) | `2 × 2` | **Doble ancho y doble alto (Grande)** | Títulos, Total, Nombre de tienda |
| **`≥ 24`** (24, 32) | Font A (Estándar) | `3 × 3` | Muy grande | Encabezados de gran tamaño |

### Respuestas HTTP del Bridge

| Código | Significado |
|---|---|
| `200 OK` | Ticket impreso correctamente |
| `400 Bad Request` | Error en estructura JSON o no hay impresora guardada |
| `500 Internal Server Error` | Error de hardware (impresora desconectada, sin papel, etc.) |

---

## 💎 Ejemplos de integración desde la Web

> ⚠️ **Importante:** La petición `POST` debe realizarse desde el **navegador del usuario (JavaScript)** hacia `http://127.0.0.1:9876/print`, ya que la impresora física y la aplicación *Miinegocio Printer* residen localmente en la computadora del cajero/usuario, no en el servidor.

---

### 1. Ruby on Rails 8 (Hotwire + Stimulus)

**Controlador Stimulus:**
```javascript
// app/javascript/controllers/printer_controller.js
import { Controller } from "@hotwired/stimulus";

export default class extends Controller {
  static values = {
    ticket: Object,
  };

  async print() {
    try {
      const response = await fetch("http://127.0.0.1:9876/print", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(this.ticketValue),
      });

      if (response.ok) {
        alert("🧾 Ticket impreso con éxito");
      } else {
        const errorText = await response.text();
        alert(`❌ Error al imprimir: ${errorText}`);
      }
    } catch (err) {
      alert("⚠️ No se pudo conectar con el puente de impresión. Asegúrate de tener abierta la aplicación Miinegocio Printer.");
    }
  }
}
```

**Vista ERB (Rails):**
```erb
<%# app/views/ventas/show.html.erb %>
<% ticket_data = {
  store_name: @venta.negocio.nombre,
  logo_url: @venta.negocio.logo_url,
  text_lines_before_items: [
    { label: "RFC: ", label_bold: true, value: @venta.negocio.rfc, value_bold: true, alignment: "space_between" },
    { label: "Folio: ", value: "##{@venta.id}", alignment: "space_between" }
  ],
  items: @venta.productos.map { |p|
    { name: p.nombre, price: p.precio.to_f, qty: p.cantidad.to_f }
  },
  subtotal: @venta.subtotal.to_f,
  iva: @venta.iva.to_f,
  total: @venta.total.to_f,
  text_lines_after_items: [
    { label: "Forma de pago: ", value: @venta.metodo_pago, alignment: "space_between" },
    { label: "¡Gracias por su compra!", alignment: "center", font_size: 14 }
  ],
  barcode: { type: "qr", value: ticket_url(@venta) }
} %>

<button
  data-controller="printer"
  data-action="click->printer#print"
  data-printer-ticket-value="<%= ticket_data.to_json %>"
  class="btn btn-primary"
>
  🧾 Imprimir Ticket
</button>
```

---

### 2. JavaScript Vanilla / Fetch

```javascript
async function imprimirTicket(ticketData) {
  try {
    const response = await fetch("http://127.0.0.1:9876/print", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(ticketData),
    });

    if (response.ok) {
      console.log("✅ Ticket enviado a la impresora");
    } else {
      const errorMsg = await response.text();
      console.error("❌ Error de impresión:", errorMsg);
    }
  } catch (error) {
    console.error("⚠️ Puente de impresión desconectado:", error);
  }
}
```

---

### 3. React / Vue / Next.js

```typescript
export async function sendTicketToPrinter(ticket: Record<string, any>): Promise<boolean> {
  try {
    const res = await fetch("http://127.0.0.1:9876/print", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(ticket),
    });

    if (!res.ok) {
      throw new Error(await res.text());
    }
    return true;
  } catch (err) {
    console.error("No se pudo imprimir el ticket:", err);
    throw err;
  }
}
```

---

## 🖥️ Desarrollo local

```bash
# Instalar dependencias
bun install

# Ejecutar en modo desarrollo
bun tauri dev

# Solo frontend (sin Rust)
bun run dev
```

---

## 📋 Requisitos para desarrollo

- [Bun](https://bun.sh/) 1.0+ (o Node.js)
- [Rust](https://www.rust-lang.org/) 1.70+
- [Tauri](https://v2.tauri.app/start/prerequisites/) 

---

## 🔧 Solución de problemas

| Problema | Solución |
|---|---|
| La app no detecta la impresora | Verifica que esté conectada por USB. Crea una regla udev si no tienes permisos (ver abajo) |
| `Error de hardware` al imprimir | La impresora puede estar desconectada o sin papel |
| App conectar al bridge | Asegúrate de que la app de escritorio esté abierta y el bridge activado (botón verde) |
| La app no muestra nombres de dispositivos | Los nombres se leen de manufactura/producto USB. Si no existen, se muestra el ID |

### Regla udev para permisos USB (Linux)

```bash
# /etc/udev/rules.d/99-usb-printers.rules
SUBSYSTEM=="usb", ATTRS{idVendor}=="xxxx", ATTRS{idProduct}=="xxxx", MODE="0666"
```

```bash
sudo udevadm control --reload-rules && sudo udevadm trigger
```
