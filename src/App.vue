<script setup>
import { ref, computed, onMounted, nextTick } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const printers = ref([]);
const selectedVendorId = ref(null);
const selectedProductId = ref(null);
const selectedDevicePath = ref(null);
const paperSize = ref("Size80mm");

const savedVendorId = ref(null);
const savedProductId = ref(null);
const savedDevicePath = ref(null);

const statusMessage = ref("");
const statusType = ref("info"); // 'info' | 'success' | 'error'
const bridgeRunning = ref(false);
const printLogs = ref([]);
const logBox = ref(null);
const expandedLogIndex = ref(null);
const copiedIndex = ref(null);

// Computadas del estado de la impresora
const isPrinterSelected = computed(() => {
  return !!(selectedVendorId.value && selectedProductId.value) || !!selectedDevicePath.value;
});

const selectedPrinterName = computed(() => {
  if (!isPrinterSelected.value) return "Sin impresora seleccionada";
  const found = printers.value.find(
    (p) =>
      (selectedVendorId.value && p.vendor_id === selectedVendorId.value && p.product_id === selectedProductId.value) ||
      (selectedDevicePath.value && p.device_path === selectedDevicePath.value)
  );
  if (found) return found.name;
  if (selectedDevicePath.value) return selectedDevicePath.value;
  if (selectedVendorId.value && selectedProductId.value) {
    return `USB (${selectedVendorId.value.toString(16)}:${selectedProductId.value.toString(16)})`;
  }
  return "Dispositivo configurado";
});

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
    statusType.value = "info";
    statusMessage.value = "Buscando impresoras conectadas...";
    printers.value = await invoke("list_printers");
    if (printers.value.length > 0) {
      statusType.value = "success";
      statusMessage.value = `Se detectaron ${printers.value.length} dispositivo(s) USB.`;
    } else {
      statusType.value = "info";
      statusMessage.value = "No se detectaron dispositivos USB conectados.";
    }
  } catch (error) {
    statusType.value = "error";
    statusMessage.value = "Error al listar impresoras: " + error;
  }
};

// Guardar configuración en archivo JSON automáticamente
const autoSavePrinterConfig = async () => {
  try {
    await invoke("save_config", {
      vendorId: selectedVendorId.value,
      productId: selectedProductId.value,
      devicePath: selectedDevicePath.value,
      paperSize: paperSize.value,
    });
    if (isPrinterSelected.value) {
      statusType.value = "success";
      statusMessage.value = `Impresora "${selectedPrinterName.value}" lista y guardada.`;
    } else {
      statusType.value = "info";
      statusMessage.value = "Configuración actualizada.";
    }
  } catch (error) {
    statusType.value = "error";
    statusMessage.value = "Error al guardar configuración: " + error;
  }
};

const handleSelectPrinter = async (event) => {
  const val = event.target.value;
  if (!val) {
    selectedVendorId.value = null;
    selectedProductId.value = null;
    selectedDevicePath.value = null;
  } else {
    const found = printers.value.find(
      (p) => `${p.vendor_id}-${p.product_id}-${p.device_path || ''}` === val
    );

    if (found) {
      selectedVendorId.value = found.vendor_id || null;
      selectedProductId.value = found.product_id || null;
      selectedDevicePath.value = found.device_path || null;
    }
  }
  await autoSavePrinterConfig();
};

const handleSelectPaperSize = async () => {
  await autoSavePrinterConfig();
};

// Reiniciar el servidor Bridge
const restartBridge = async () => {
  try {
    statusType.value = "info";
    statusMessage.value = "Reiniciando bridge...";
    const result = await invoke("restart_bridge");
    statusType.value = "success";
    statusMessage.value = result;
  } catch (error) {
    statusType.value = "error";
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
    statusType.value = "info";
    statusMessage.value = "Imprimiendo hoja de prueba...";
    await invoke("print_test_page", {
      vendorId: selectedVendorId.value,
      productId: selectedProductId.value,
      devicePath: selectedDevicePath.value,
      paperSize: paperSize.value,
    });
    statusType.value = "success";
    statusMessage.value = "¡Hoja de prueba enviada con éxito!";
  } catch (error) {
    statusType.value = "error";
    statusMessage.value = "Error de impresión: " + error;
  }
};

// Imprimir Ticket de Prueba Completo (formato universal)
const testPrintFull = async () => {
  try {
    statusType.value = "info";
    statusMessage.value = "Imprimiendo ticket de prueba completo...";
    await invoke("print_test_ticket", {
      vendorId: selectedVendorId.value,
      productId: selectedProductId.value,
      devicePath: selectedDevicePath.value,
      paperSize: paperSize.value,
    });
    statusType.value = "success";
    statusMessage.value = "¡Ticket de prueba completo impreso!";
  } catch (error) {
    statusType.value = "error";
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
      statusType.value = "success";
      statusMessage.value = "Bridge activo — escuchando peticiones en puerto 9876";
    } else {
      statusType.value = "error";
      statusMessage.value = "Bridge detenido";
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
  <main class="app-layout">
    <!-- Header -->
    <header class="app-header">
      <div class="brand-badge">
        <span class="brand-icon">⚡</span>
        <span class="brand-text">Miinegocio</span>
      </div>
      <h1 class="app-title">POS Thermal Bridge</h1>
      <p class="app-subtitle">Servidor de impresión local para aplicaciones web</p>
    </header>

    <!-- Semáforos de Estado Dual (Servidor HTTP & Impresora) -->
    <section class="status-grid">
      <!-- Semáforo 1: Servidor HTTP Bridge -->
      <div class="status-card" :class="bridgeRunning ? 'status-card-online' : 'status-card-offline'">
        <div class="semaphore-container">
          <span class="semaphore-ring"></span>
          <span class="semaphore-dot" :class="bridgeRunning ? 'dot-green' : 'dot-red'"></span>
        </div>
        <div class="status-content">
          <div class="status-label">Servidor Bridge HTTP</div>
          <div class="status-value">
            {{ bridgeRunning ? 'Puerto 9876 Activo' : 'Servidor Inactivo' }}
          </div>
        </div>
        <div class="status-pill" :class="bridgeRunning ? 'pill-green' : 'pill-red'">
          {{ bridgeRunning ? 'ONLINE' : 'OFFLINE' }}
        </div>
      </div>

      <!-- Semáforo 2: Impresora Térmica -->
      <div 
        class="status-card" 
        :class="isPrinterSelected ? 'status-card-online' : 'status-card-offline'"
      >
        <div class="semaphore-container">
          <span class="semaphore-ring"></span>
          <span 
            class="semaphore-dot" 
            :class="isPrinterSelected ? 'dot-green' : 'dot-red'"
          ></span>
        </div>
        <div class="status-content">
          <div class="status-label">Impresora Térmica</div>
          <div class="status-value truncate" :title="selectedPrinterName">
            {{ selectedPrinterName }}
          </div>
        </div>
        <div 
          class="status-pill" 
          :class="isPrinterSelected ? 'pill-green' : 'pill-red'"
        >
          {{ isPrinterSelected ? 'LISTA' : 'NO ASIGNADA' }}
        </div>
      </div>
    </section>

    <!-- Panel de Configuración -->
    <section class="glass-panel">
      <div class="panel-header">
        <span class="panel-icon">⚙️</span>
        <h2 class="panel-title">Configuración de Hardware</h2>
      </div>

      <div class="form-grid">
        <!-- 1. Selección de Impresora -->
        <div class="form-group">
          <div class="form-label-row">
            <label for="printer-select">1. Impresora USB:</label>
            <button @click="fetchPrinters" class="link-btn">
              🔄 Refrescar
            </button>
          </div>
          <div class="select-wrapper">
            <select 
              id="printer-select"
              :value="selectedVendorId ? `${selectedVendorId}-${selectedProductId}-${selectedDevicePath || ''}` : ''"
              @change="handleSelectPrinter"
              class="custom-select"
            >
              <option value="">-- Selecciona una impresora térmica --</option>
              <option 
                v-for="printer in printers" 
                :key="`${printer.vendor_id}-${printer.product_id}-${printer.device_path || ''}`"
                :value="`${printer.vendor_id}-${printer.product_id}-${printer.device_path || ''}`"
              >
                {{ printer.name }} (ID: {{ printer.vendor_id.toString(16).padStart(4, '0') }}:{{ printer.product_id.toString(16).padStart(4, '0') }})
              </option>
            </select>
          </div>
        </div>

        <!-- 2. Tamaño de Papel -->
        <div class="form-group">
          <label for="paper-select">2. Ancho del Papel:</label>
          <div class="select-wrapper">
            <select id="paper-select" v-model="paperSize" @change="handleSelectPaperSize" class="custom-select">
              <option value="Size80mm">80 mm (Estándar - 64/48 car.)</option>
              <option value="Size60mm">60 mm / 58 mm (Compacta - 42/32 car.)</option>
            </select>
          </div>
        </div>
      </div>

      <!-- Botones de Acción -->
      <div class="actions-row">
        <button @click="testPrint" class="btn btn-secondary">
          <span class="btn-icon">📄</span> Prueba Simple
        </button>
        <button @click="testPrintFull" class="btn btn-secondary">
          <span class="btn-icon">🧾</span> Ticket Completo
        </button>
        <button @click="restartBridge" class="btn btn-neutral">
          <span class="btn-icon">🔄</span> Reiniciar Bridge
        </button>
      </div>

      <!-- Banner de Estado -->
      <transition name="fade">
        <div v-if="statusMessage" class="status-banner" :class="`banner-${statusType}`">
          <span class="banner-icon">
            {{ statusType === 'success' ? '✅' : (statusType === 'error' ? '❌' : 'ℹ️') }}
          </span>
          <span class="banner-text">{{ statusMessage }}</span>
        </div>
      </transition>
    </section>

    <!-- Sección de Formato Universal -->
    <details class="details-card">
      <summary class="details-summary">
        <span class="summary-left">
          <span class="summary-icon">📐</span>
          <strong>Especificación JSON & API Bridge</strong>
        </span>
        <span class="summary-hint">POST a http://127.0.0.1:9876/print</span>
      </summary>
      <div class="details-body">
        <p class="details-desc">
          Realiza una petición <code>POST</code> desde JavaScript de tu sitio web a <code>http://127.0.0.1:9876/print</code>:
        </p>
        <pre class="json-code-box">{
  "store_name": "Mi Negocio",
  "logo_url": "https://ejemplo.com/logo.png",
  "text_lines_before_items": [
    { "label": "RFC: ", "label_bold": true, "value": "XAXX010101000",
      "value_bold": true, "font_size": "small", "alignment": "space_between" }
  ],
  "items": [
    { "name": "Café Americano", "price": 45.00, "qty": 2 }
  ],
  "subtotal": 45.00,
  "iva": 7.20,
  "total": 52.20,
  "text_lines_after_items": [
    { "label": "¡Gracias por su compra!", "font_size": "medium", "alignment": "center" }
  ],
  "barcode": { "type": "qr", "value": "https://ejemplo.com/ticket/1", "alignment": "center" }
}</pre>
        <p class="details-footer">
          Consulta el archivo <code>README.md</code> para conocer todos los parámetros detallados.
        </p>
      </div>
    </details>

    <!-- Historial de Impresiones en Tiempo Real -->
    <section class="log-section">
      <div class="log-topbar">
        <div class="log-title-wrap">
          <span class="log-main-icon">📋</span>
          <h3 class="log-heading">Historial de Impresión</h3>
          <span class="log-count-tag">{{ printLogs.length }} eventos</span>
        </div>
        <button @click="fetchPrintHistory" class="link-btn">
          🔄 Actualizar
        </button>
      </div>

      <div ref="logBox" class="terminal-box">
        <div v-if="printLogs.length === 0" class="terminal-empty">
          <span class="empty-icon">🖨️</span>
          <p>Esperando solicitudes de impresión...</p>
          <span class="empty-sub">Las solicitudes a http://127.0.0.1:9876 aparecerán aquí en vivo</span>
        </div>

        <div
          v-for="(log, i) in printLogs"
          :key="i"
          class="terminal-row-wrapper"
          :class="{ 'row-has-error': !log.success }"
        >
          <div class="terminal-row" @click="toggleExpandLog(i)">
            <span class="row-arrow">{{ expandedLogIndex === i ? '▼' : '▶' }}</span>
            <span class="row-time">{{ log.timestamp }}</span>
            <span class="row-source">{{ log.source }}</span>
            <span class="row-store truncate">{{ log.store_name }}</span>
            <span class="row-meta">
              <template v-if="log.item_count > 0">{{ log.item_count }} items · </template>
              {{ log.total }}
            </span>
            <span class="row-status">
              <span v-if="log.success" class="status-dot-sm dot-green" title="Éxito"></span>
              <span v-else class="status-dot-sm dot-red" title="Error"></span>
            </span>
          </div>

          <!-- Payload JSON Expandible -->
          <div v-if="expandedLogIndex === i" class="terminal-detail">
            <div v-if="log.error_message" class="error-banner">
              ⚠️ <strong>Error de hardware/sistema:</strong> {{ log.error_message }}
            </div>

            <div class="json-header">
              <span>Payload JSON procesado:</span>
              <button
                v-if="log.raw_json"
                @click="copyJson(log.raw_json, i, $event)"
                class="btn-copy-json"
              >
                {{ copiedIndex === i ? '✅ Copiado' : '📋 Copiar' }}
              </button>
            </div>

            <pre v-if="log.raw_json" class="json-content-viewer">{{ log.raw_json }}</pre>
            <div v-else class="json-empty-note">
              (Impresión local sin payload JSON externo)
            </div>
          </div>
        </div>
      </div>
    </section>
  </main>
</template>

<style scoped>
/* Reset & Layout Base */
.app-layout {
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
  max-width: 720px;
  margin: 0 auto;
  padding: 24px 20px 48px;
  color: #1e293b;
  -webkit-font-smoothing: antialiased;
}

/* Header */
.app-header {
  text-align: center;
  margin-bottom: 24px;
}
.brand-badge {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 4px 12px;
  background: rgba(37, 99, 235, 0.08);
  border: 1px solid rgba(37, 99, 235, 0.16);
  border-radius: 9999px;
  font-size: 12px;
  font-weight: 600;
  color: #2563eb;
  margin-bottom: 8px;
}
.app-title {
  font-size: 26px;
  font-weight: 800;
  letter-spacing: -0.02em;
  color: #0f172a;
  margin: 4px 0;
}
.app-subtitle {
  font-size: 14px;
  color: #64748b;
  margin: 0;
}

/* Semáforos Duales (Grid) */
.status-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
  gap: 12px;
  margin-bottom: 20px;
}

.status-card {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 16px;
  border-radius: 12px;
  background: #ffffff;
  border: 1px solid #e2e8f0;
  box-shadow: 0 4px 12px rgba(15, 23, 42, 0.04);
  transition: all 0.3s ease;
}
.status-card-online {
  border-color: rgba(16, 185, 129, 0.3);
  background: linear-gradient(135deg, #ffffff 0%, #f0fdf4 100%);
}
.status-card-warning {
  border-color: rgba(245, 158, 11, 0.35);
  background: linear-gradient(135deg, #ffffff 0%, #fffbeb 100%);
}
.status-card-offline {
  border-color: rgba(239, 68, 68, 0.25);
  background: linear-gradient(135deg, #ffffff 0%, #fef2f2 100%);
}

.semaphore-container {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  flex-shrink: 0;
}
.semaphore-dot {
  width: 12px;
  height: 12px;
  border-radius: 50%;
  transition: all 0.3s cubic-bezier(0.4, 0, 0.2, 1);
}
.dot-green {
  background-color: #10b981;
  box-shadow: 0 0 10px rgba(16, 185, 129, 0.7);
}
.dot-amber {
  background-color: #f59e0b;
  box-shadow: 0 0 10px rgba(245, 158, 11, 0.7);
}
.dot-red {
  background-color: #ef4444;
  box-shadow: 0 0 8px rgba(239, 68, 68, 0.5);
}

.status-content {
  flex: 1;
  min-width: 0;
}
.status-label {
  font-size: 11px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: #64748b;
  margin-bottom: 2px;
}
.status-value {
  font-size: 13px;
  font-weight: 600;
  color: #1e293b;
}

.status-pill {
  font-size: 10px;
  font-weight: 800;
  letter-spacing: 0.05em;
  padding: 3px 8px;
  border-radius: 6px;
  flex-shrink: 0;
}
.pill-green {
  background: rgba(16, 185, 129, 0.15);
  color: #047857;
  border: 1px solid rgba(16, 185, 129, 0.3);
}
.pill-amber {
  background: rgba(245, 158, 11, 0.15);
  color: #b45309;
  border: 1px solid rgba(245, 158, 11, 0.3);
}
.pill-red {
  background: rgba(239, 68, 68, 0.12);
  color: #b91c1c;
  border: 1px solid rgba(239, 68, 68, 0.25);
}

/* Panel de Configuración Glassmorphism */
.glass-panel {
  background: #ffffff;
  border: 1px solid #e2e8f0;
  border-radius: 16px;
  padding: 20px;
  box-shadow: 0 10px 30px -5px rgba(15, 23, 42, 0.05);
  margin-bottom: 20px;
}

.panel-header {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 18px;
  padding-bottom: 12px;
  border-bottom: 1px solid #f1f5f9;
}
.panel-icon {
  font-size: 18px;
}
.panel-title {
  font-size: 16px;
  font-weight: 700;
  color: #0f172a;
  margin: 0;
}

.form-grid {
  display: grid;
  grid-template-columns: 1fr;
  gap: 16px;
  margin-bottom: 20px;
}
@media (min-width: 600px) {
  .form-grid {
    grid-template-columns: 3fr 2fr;
  }
}

.form-group {
  display: flex;
  flex-direction: column;
}
.form-label-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 6px;
}
label {
  font-size: 13px;
  font-weight: 600;
  color: #334155;
  margin-bottom: 6px;
}

.select-wrapper {
  position: relative;
}
.custom-select {
  width: 100%;
  padding: 10px 12px;
  font-size: 13px;
  color: #1e293b;
  background-color: #f8fafc;
  border: 1px solid #cbd5e1;
  border-radius: 8px;
  outline: none;
  transition: all 0.2s;
  box-sizing: border-box;
}
.custom-select:focus {
  background-color: #ffffff;
  border-color: #3b82f6;
  box-shadow: 0 0 0 3px rgba(59, 130, 246, 0.15);
}

.link-btn {
  background: none;
  border: none;
  color: #2563eb;
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  padding: 2px 6px;
  border-radius: 4px;
  transition: background 0.15s;
}
.link-btn:hover {
  background: rgba(37, 99, 235, 0.08);
}

/* Botones de Acción */
.actions-row {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  margin-bottom: 14px;
}
.btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 9px 14px;
  border-radius: 8px;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  border: 1px solid transparent;
  transition: all 0.2s cubic-bezier(0.4, 0, 0.2, 1);
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.05);
}
.btn:hover {
  transform: translateY(-1px);
}
.btn:active {
  transform: translateY(0);
}

.btn-save {
  background: #10b981;
  color: #ffffff;
  border-color: #059669;
}
.btn-save:hover {
  background: #059669;
  box-shadow: 0 4px 12px rgba(16, 185, 129, 0.3);
}
.btn-highlight {
  animation: pulse-border 1.5s infinite;
}
@keyframes pulse-border {
  0%, 100% { box-shadow: 0 0 0 0 rgba(16, 185, 129, 0.6); }
  50% { box-shadow: 0 0 0 6px rgba(16, 185, 129, 0.2); }
}

.btn-secondary {
  background: #f1f5f9;
  color: #334155;
  border-color: #cbd5e1;
}
.btn-secondary:hover {
  background: #e2e8f0;
  color: #0f172a;
}

.btn-neutral {
  background: #f8fafc;
  color: #475569;
  border-color: #e2e8f0;
}
.btn-neutral:hover {
  background: #f1f5f9;
  color: #1e293b;
}

/* Banner de Estado */
.status-banner {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 14px;
  border-radius: 8px;
  font-size: 13px;
  font-weight: 500;
  margin-top: 14px;
}
.banner-success {
  background: #f0fdf4;
  color: #166534;
  border: 1px solid #bbf7d0;
}
.banner-error {
  background: #fef2f2;
  color: #991b1b;
  border: 1px solid #fecaca;
}
.banner-info {
  background: #f8fafc;
  color: #334155;
  border: 1px solid #e2e8f0;
}

/* Details Card (Formato) */
.details-card {
  background: #ffffff;
  border: 1px solid #e2e8f0;
  border-radius: 12px;
  padding: 12px 16px;
  margin-bottom: 24px;
}
.details-summary {
  display: flex;
  justify-content: space-between;
  align-items: center;
  cursor: pointer;
  font-size: 13px;
  color: #334155;
  user-select: none;
}
.summary-left {
  display: flex;
  align-items: center;
  gap: 6px;
}
.summary-hint {
  font-size: 11px;
  color: #64748b;
  font-family: monospace;
}
.details-body {
  margin-top: 12px;
  padding-top: 12px;
  border-top: 1px dashed #e2e8f0;
}
.details-desc {
  font-size: 12px;
  color: #64748b;
  margin: 0 0 8px 0;
}
.details-footer {
  font-size: 12px;
  color: #94a3b8;
  margin: 8px 0 0 0;
}

.json-code-box {
  background: #0f172a;
  color: #38bdf8;
  padding: 12px;
  border-radius: 8px;
  font-family: "SF Mono", "Fira Code", monospace;
  font-size: 11px;
  overflow-x: auto;
  line-height: 1.5;
  margin: 0;
}

/* Historial Terminal */
.log-section {
  margin-top: 24px;
}
.log-topbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 10px;
}
.log-title-wrap {
  display: flex;
  align-items: center;
  gap: 8px;
}
.log-heading {
  font-size: 15px;
  font-weight: 700;
  color: #0f172a;
  margin: 0;
}
.log-count-tag {
  font-size: 11px;
  background: #e2e8f0;
  color: #475569;
  padding: 2px 6px;
  border-radius: 4px;
  font-weight: 600;
}

.terminal-box {
  background: #0f172a;
  border-radius: 12px;
  border: 1px solid #1e293b;
  max-height: 320px;
  overflow-y: auto;
  box-shadow: inset 0 2px 8px rgba(0, 0, 0, 0.4);
}
.terminal-empty {
  text-align: center;
  padding: 32px 16px;
  color: #64748b;
}
.empty-icon {
  font-size: 28px;
  display: block;
  margin-bottom: 6px;
}
.empty-sub {
  font-size: 11px;
  color: #475569;
}

.terminal-row-wrapper {
  border-bottom: 1px solid #1e293b;
}
.terminal-row-wrapper:last-child {
  border-bottom: none;
}
.row-has-error .terminal-row {
  background: rgba(239, 68, 68, 0.1);
}

.terminal-row {
  display: grid;
  grid-template-columns: 16px 70px 85px 1fr auto 16px;
  gap: 8px;
  align-items: center;
  padding: 9px 12px;
  font-family: "SF Mono", "Fira Code", monospace;
  font-size: 11px;
  color: #94a3b8;
  cursor: pointer;
  transition: background 0.15s;
}
.terminal-row:hover {
  background: rgba(255, 255, 255, 0.05);
}

.row-arrow { color: #64748b; font-size: 9px; }
.row-time { color: #64748b; }
.row-source { color: #38bdf8; font-weight: 600; }
.row-store { color: #f8fafc; font-weight: 500; }
.row-meta { color: #94a3b8; text-align: right; }
.status-dot-sm {
  display: inline-block;
  width: 8px;
  height: 8px;
  border-radius: 50%;
}

.terminal-detail {
  background: #090d16;
  padding: 12px;
  border-top: 1px solid #1e293b;
}
.error-banner {
  background: rgba(239, 68, 68, 0.15);
  border: 1px solid rgba(239, 68, 68, 0.3);
  color: #fca5a5;
  padding: 8px 10px;
  border-radius: 6px;
  font-size: 11px;
  margin-bottom: 8px;
}
.json-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  font-size: 11px;
  color: #94a3b8;
  margin-bottom: 6px;
}
.btn-copy-json {
  background: #1e293b;
  color: #f8fafc;
  border: 1px solid #334155;
  padding: 3px 8px;
  border-radius: 4px;
  font-size: 10px;
  font-weight: 600;
  cursor: pointer;
}
.btn-copy-json:hover {
  background: #334155;
}
.json-content-viewer {
  background: #020617;
  color: #4ade80;
  padding: 10px;
  border-radius: 6px;
  font-size: 10px;
  max-height: 180px;
  overflow-y: auto;
  margin: 0;
  line-height: 1.4;
}
.json-empty-note {
  color: #64748b;
  font-size: 11px;
  font-style: italic;
}

.truncate {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* Animaciones */
.fade-enter-active, .fade-leave-active {
  transition: opacity 0.2s, transform 0.2s;
}
.fade-enter-from, .fade-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}
</style>