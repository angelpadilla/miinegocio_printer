<script setup>
import { ref, onMounted, nextTick } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const printers = ref([]);
const selectedVendorId = ref(null);
const selectedProductId = ref(null);
const selectedDevicePath = ref(null);
const paperSize = ref("Size80mm");

const statusMessage = ref("");
const bridgeRunning = ref(false);
const printLogs = ref([]);
const logBox = ref(null);
const expandedLogIndex = ref(null);
const copiedIndex = ref(null);

// Scroll automático al final del historial
const scrollLogToBottom = async () => {
  await nextTick();
  if (logBox.value) {
    logBox.value.scrollTop = logBox.value.scrollHeight;
  }
};

// Cargar la configuración guardada desde el backend (JSON)
const loadSavedConfig = async () => {
  try {
    const config = await invoke("get_config");
    if (config) {
      if (config.vendor_id !== undefined) selectedVendorId.value = config.vendor_id;
      if (config.product_id !== undefined) selectedProductId.value = config.product_id;
      if (config.device_path) selectedDevicePath.value = config.device_path;
      if (config.paper_size) paperSize.value = config.paper_size;
    }
  } catch (error) {
    console.error("Error al cargar la configuración:", error);
  }
};

// Buscar impresoras USB al cargar la app
const fetchPrinters = async () => {
  try {
    statusMessage.value = "Buscando impresoras...";
    printers.value = await invoke("list_printers");
    if (printers.value.length > 0) {
      statusMessage.value = `Se encontraron ${printers.value.length} dispositivos.`;
    } else {
      statusMessage.value = "No se detectaron dispositivos USB.";
    }
  } catch (error) {
    statusMessage.value = "Error al listar impresoras: " + error;
  }
};

const handleSelectPrinter = (event) => {
  const val = event.target.value;
  if (!val) {
    selectedVendorId.value = null;
    selectedProductId.value = null;
    selectedDevicePath.value = null;
    return;
  }

  const found = printers.value.find(
    (p) => `${p.vendor_id}-${p.product_id}-${p.device_path || ''}` === val
  );

  if (found) {
    selectedVendorId.value = found.vendor_id || null;
    selectedProductId.value = found.product_id || null;
    selectedDevicePath.value = found.device_path || null;
  }
};

// Guardar configuración en archivo JSON
const savePrinterConfig = async () => {
  try {
    await invoke("save_config", {
      vendorId: selectedVendorId.value,
      productId: selectedProductId.value,
      devicePath: selectedDevicePath.value,
      paperSize: paperSize.value,
    });
    statusMessage.value = "¡Configuración guardada exitosamente en JSON!";
  } catch (error) {
    statusMessage.value = "Error al guardar configuración: " + error;
  }
};

// Reiniciar el servidor Bridge
const restartBridge = async () => {
  try {
    statusMessage.value = "Reiniciando bridge...";
    const result = await invoke("restart_bridge");
    statusMessage.value = result;
  } catch (error) {
    statusMessage.value = "Error al reiniciar bridge: " + error;
  }
};

// Consultar estado del bridge
const checkBridgeStatus = async () => {
  try {
    bridgeRunning.value = await invoke("bridge_status");
  } catch {
    bridgeRunning.value = false;
  }
};

// Imprimir Hoja de Prueba
const testPrint = async () => {
  try {
    statusMessage.value = "Imprimiendo hoja de prueba...";
    await invoke("print_test_page", {
      vendorId: selectedVendorId.value,
      productId: selectedProductId.value,
      devicePath: selectedDevicePath.value,
      paperSize: paperSize.value,
    });
    statusMessage.value = "¡Hoja de prueba impresa!";
  } catch (error) {
    statusMessage.value = "Error de impresión: " + error;
  }
};

// Imprimir Ticket de Prueba Completo (formato universal)
const testPrintFull = async () => {
  try {
    statusMessage.value = "Imprimiendo ticket de prueba completo...";
    await invoke("print_test_ticket", {
      vendorId: selectedVendorId.value,
      productId: selectedProductId.value,
      devicePath: selectedDevicePath.value,
      paperSize: paperSize.value,
    });
    statusMessage.value = "¡Ticket de prueba completo impreso!";
  } catch (error) {
    statusMessage.value = "Error de impresión: " + error;
  }
};

// Obtener historial de impresiones
const fetchPrintHistory = async () => {
  try {
    printLogs.value = await invoke("get_print_history");
    await scrollLogToBottom();
  } catch {
    printLogs.value = [];
  }
};

const toggleExpandLog = (index) => {
  expandedLogIndex.value = expandedLogIndex.value === index ? null : index;
};

const copyJson = async (jsonStr, index, event) => {
  event?.stopPropagation();
  if (!jsonStr) return;
  try {
    await navigator.clipboard.writeText(jsonStr);
    copiedIndex.value = index;
    setTimeout(() => {
      if (copiedIndex.value === index) copiedIndex.value = null;
    }, 2000);
  } catch (err) {
    console.error("Error al copiar JSON:", err);
  }
};

onMounted(async () => {
  await loadSavedConfig();
  await fetchPrinters();
  await checkBridgeStatus();
  await fetchPrintHistory();

  // Escuchar cambios de estado del bridge emitidos por el backend
  await listen("bridge-status", (event) => {
    bridgeRunning.value = !!event.payload;
    if (bridgeRunning.value) {
      statusMessage.value = "✅ Bridge iniciado — escuchando en puerto 9876";
    } else {
      statusMessage.value = "⚠️ Bridge detenido";
    }
  });

  // Escuchar nuevas entradas de historial de impresión
  await listen("print-log", (event) => {
    printLogs.value.push(event.payload);
    expandedLogIndex.value = printLogs.value.length - 1;
    scrollLogToBottom();
  });
});
</script>

<template>
  <main class="container">
    <h2>POS Printer 🚀</h2>
    <p>Configura tu impresora térmica para recibir tickets de la app web.</p>

    <!-- Semáforo del Bridge -->
    <div class="bridge-indicator">
      <span class="semaphore" :class="{ active: bridgeRunning }"></span>
      <span class="bridge-label">
        Bridge {{ bridgeRunning ? 'activo' : 'inactivo' }} (Puerto 9876)
      </span>
    </div>

    <div class="form-group">
      <label>1. Selecciona tu Impresora:</label>
      <div class="select-row">
        <select 
          :value="selectedVendorId ? `${selectedVendorId}-${selectedProductId}-${selectedDevicePath || ''}` : ''"
          @change="handleSelectPrinter"
        >
          <option value="">-- Selecciona un dispositivo --</option>
          <option 
            v-for="printer in printers" 
            :key="`${printer.vendor_id}-${printer.product_id}-${printer.device_path || ''}`"
            :value="`${printer.vendor_id}-${printer.product_id}-${printer.device_path || ''}`"
          >
            {{ printer.name }} (ID: {{ printer.vendor_id.toString(16) }}:{{ printer.product_id.toString(16) }})
          </option>
        </select>
        <button @click="fetchPrinters" class="btn-refresh">🔄 Refrescar Lista</button>
      </div>
    </div>

    <div class="form-group">
      <label>2. Tamaño del Papel:</label>
      <select v-model="paperSize">
        <option value="Size80mm">80mm (Estándar Ancho)</option>
        <option value="Size60mm">60mm / 58mm (Compacta)</option>
      </select>
    </div>

    <div class="actions">
      <button @click="savePrinterConfig" class="btn-secondary">💾 Guardar Configuración</button>
      <button @click="testPrint" class="btn-secondary">📄 Hoja de Prueba</button>
      <button @click="testPrintFull" class="btn-secondary">🧾 Hoja de Prueba 2</button>
      <button @click="restartBridge" class="btn-primary">🔄 Reiniciar Bridge</button>
    </div>

    <div v-if="statusMessage" class="status-box">
      <strong>Estado:</strong> {{ statusMessage }}
    </div>

    <!-- Formato Universal -->
    <details class="format-section">
      <summary>📐 Formato Universal del JSON</summary>
      <div class="format-content">
        <p>Envía un <code>POST</code> a <code>http://127.0.0.1:9876/print</code> con este formato:</p>
        <pre class="format-json">{
  "store_name": "Mi Negocio",
  "logo_url": "https://ejemplo.com/logo.png",
  "text_lines_before_items": [
    { "label": "RFC: ", "label_bold": true, "value": "XAXX010101000",
      "value_bold": true, "alignment": "space_between" }
  ],
  "items": [
    { "name": "Producto", "price": 45.00, "qty": 2 }
  ],
  "subtotal": 45.00,
  "iva": 7.20,
  "total": 52.20,
  "text_lines_after_items": [
    { "label": "Gracias", "alignment": "center", "font_size": 14 }
  ],
  "barcode": { "type": "qr", "value": "https://ejemplo.com/ticket/1", "alignment": "center" }
}</pre>
        <p>Ver <code>README.md</code> para documentación completa.</p>
      </div>
    </details>

    <!-- Historial de impresión -->
    <div class="log-section">
      <div class="log-header">
        <strong>📋 Historial de Impresiones</strong>
        <span class="log-hint">Haz clic en un registro para ver su JSON</span>
        <button @click="fetchPrintHistory" class="btn-refresh">🔄 Refrescar</button>
      </div>
      <div ref="logBox" class="log-box">
        <div v-if="printLogs.length === 0" class="log-empty">
          Sin actividad aún. Las impresiones aparecerán aquí.
        </div>
        <div
          v-for="(log, i) in printLogs"
          :key="i"
          class="log-card"
          :class="{ 'log-card-error': !log.success }"
        >
          <div class="log-entry" @click="toggleExpandLog(i)">
            <span class="log-toggle-icon">{{ expandedLogIndex === i ? '▼' : '▶' }}</span>
            <span class="log-time">{{ log.timestamp }}</span>
            <span class="log-source">{{ log.source }}</span>
            <span class="log-store">{{ log.store_name }}</span>
            <span class="log-meta">
              <template v-if="log.item_count > 0">{{ log.item_count }} items · </template>
              {{ log.total }}
            </span>
            <span v-if="!log.success" class="log-badge" title="Error">❌</span>
            <span v-else class="log-badge" title="Éxito">✅</span>
          </div>

          <!-- Detalle expandible con el JSON recibido -->
          <div v-if="expandedLogIndex === i" class="log-detail">
            <div v-if="log.error_message" class="log-error-msg">
              <strong>Error de hardware/sistema:</strong> {{ log.error_message }}
            </div>

            <div class="log-json-header">
              <span><strong>JSON recibido:</strong></span>
              <button
                v-if="log.raw_json"
                @click="copyJson(log.raw_json, i, $event)"
                class="btn-copy"
              >
                {{ copiedIndex === i ? '✅ Copiado' : '📋 Copiar JSON' }}
              </button>
            </div>

            <pre v-if="log.raw_json" class="log-json-content">{{ log.raw_json }}</pre>
            <div v-else class="log-no-json">
              (Sin payload JSON registrado para esta acción local)
            </div>
          </div>
        </div>
      </div>
    </div>
  </main>
</template>

<style scoped>
.container {
  font-family: Arial, sans-serif;
  padding: 20px;
  max-width: 650px;
  margin: 0 auto;
}

/* Semáforo del Bridge */
.bridge-indicator {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 14px;
  background: #fafafa;
  border: 1px solid #e0e0e0;
  border-radius: 8px;
  margin-bottom: 20px;
}
.semaphore {
  width: 16px;
  height: 16px;
  border-radius: 50%;
  background-color: #e53935;
  box-shadow: 0 0 6px rgba(229, 57, 53, 0.5);
  transition: background-color 0.3s, box-shadow 0.3s;
}
.semaphore.active {
  background-color: #43a047;
  box-shadow: 0 0 8px rgba(67, 160, 71, 0.6);
}
.bridge-label {
  font-weight: 600;
  font-size: 14px;
  color: #333;
}

.form-group {
  margin-bottom: 15px;
  display: flex;
  flex-direction: column;
}
.select-row {
  display: flex;
  gap: 8px;
  align-items: center;
}
.select-row select {
  flex: 1;
}
label {
  font-weight: bold;
  margin-bottom: 5px;
}
select {
  padding: 8px;
  font-size: 14px;
  border-radius: 4px;
  border: 1px solid #ccc;
}
.btn-refresh {
  background: none;
  border: none;
  color: #0066cc;
  cursor: pointer;
  font-size: 13px;
  white-space: nowrap;
}
.actions {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  margin-top: 20px;
}
button {
  padding: 10px 15px;
  border-radius: 4px;
  border: none;
  cursor: pointer;
  font-weight: bold;
}
.btn-primary { background-color: #4caf50; color: white; }
.btn-secondary { background-color: #2196f3; color: white; }
.status-box {
  margin-top: 20px;
  padding: 10px;
  background-color: #f0f0f0;
  border-left: 4px solid #333;
}

/* Formato Universal */
.format-section {
  margin-top: 20px;
  background: #f8f9fa;
  border: 1px solid #dee2e6;
  border-radius: 8px;
  padding: 12px 16px;
  cursor: pointer;
}
.format-section summary {
  font-weight: 600;
  font-size: 14px;
  color: #333;
  cursor: pointer;
}
.format-content {
  margin-top: 12px;
  font-size: 13px;
  color: #555;
}
.format-content p {
  margin: 6px 0;
}
.format-json {
  background: #1e1e1e;
  color: #d4d4d4;
  padding: 12px;
  border-radius: 6px;
  font-family: "SF Mono", "Fira Code", monospace;
  font-size: 11px;
  overflow-x: auto;
  white-space: pre;
  line-height: 1.5;
}

/* Historial de impresión */
.log-section {
  margin-top: 24px;
}
.log-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 8px;
}
.log-hint {
  font-size: 12px;
  color: #777;
}
.log-box {
  max-height: 320px;
  overflow-y: auto;
  background: #1e1e1e;
  border-radius: 6px;
  padding: 8px;
  font-family: "SF Mono", "Fira Code", monospace;
  font-size: 12px;
}
.log-empty {
  color: #888;
  text-align: center;
  padding: 20px 0;
}
.log-card {
  border-bottom: 1px solid #333;
}
.log-card:last-child {
  border-bottom: none;
}
.log-card-error .log-entry {
  background: rgba(229, 57, 53, 0.12);
}
.log-entry {
  display: grid;
  grid-template-columns: 18px 85px 95px 1fr auto 24px;
  gap: 6px;
  align-items: center;
  padding: 7px 6px;
  color: #ccc;
  cursor: pointer;
  transition: background 0.15s;
  border-radius: 4px;
}
.log-entry:hover {
  background: rgba(255, 255, 255, 0.06);
}
.log-toggle-icon {
  font-size: 10px;
  color: #888;
  text-align: center;
}
.log-time { color: #888; font-size: 11px; }
.log-source { color: #64b5f6; font-weight: 600; font-size: 11px; }
.log-store { color: #fff; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.log-meta { color: #aaa; text-align: right; }
.log-badge { text-align: center; font-size: 12px; }

/* Detalle expandible del JSON */
.log-detail {
  padding: 10px 12px;
  background: #181818;
  border-top: 1px dashed #333;
  margin-bottom: 4px;
  border-radius: 0 0 4px 4px;
}
.log-error-msg {
  color: #ff6b6b;
  background: rgba(229, 57, 53, 0.15);
  padding: 6px 10px;
  border-radius: 4px;
  margin-bottom: 8px;
  font-size: 11px;
}
.log-json-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 6px;
  color: #aaa;
  font-size: 11px;
}
.btn-copy {
  background: #333;
  color: #eee;
  border: 1px solid #555;
  padding: 3px 8px;
  border-radius: 3px;
  font-size: 11px;
  cursor: pointer;
  transition: background 0.2s;
}
.btn-copy:hover {
  background: #444;
}
.log-json-content {
  background: #111;
  color: #81c784;
  padding: 10px;
  border-radius: 4px;
  font-size: 11px;
  max-height: 200px;
  overflow-x: auto;
  overflow-y: auto;
  white-space: pre;
  line-height: 1.4;
  margin: 0;
}
.log-no-json {
  color: #777;
  font-style: italic;
  font-size: 11px;
}
</style>