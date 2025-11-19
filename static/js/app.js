// Global state
let config = {
    mqtt_servers: {},
    mqtt_monitors: {},
    alpaca_monitors: {},
    server_port: 8080,
    device_name: "LLAMA Safety Monitor"
};

let currentEditId = null;
let currentEditType = null;

// Initialize
document.addEventListener('DOMContentLoaded', async () => {
    await loadConfig();
    await refreshStatus();
    // Auto-refresh status every 5 seconds
    setInterval(refreshStatus, 5000);
});

// Tab switching
function switchTab(tabName) {
    // Update tab buttons
    document.querySelectorAll('.tab').forEach(tab => {
        tab.classList.remove('active');
    });
    event.target.classList.add('active');

    // Update tab content
    document.querySelectorAll('.tab-content').forEach(content => {
        content.classList.remove('active');
    });
    document.getElementById(`tab-${tabName}`).classList.add('active');
}

// Load configuration
async function loadConfig() {
    try {
        const response = await fetch('/api/config');
        config = await response.json();
        renderAll();
    } catch (error) {
        showNotification('Failed to load configuration', 'error');
        console.error(error);
    }
}

// Save configuration
async function saveConfig() {
    try {
        const response = await fetch('/api/config', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(config)
        });

        if (response.ok) {
            showNotification('Configuration saved successfully', 'success');
            return true;
        } else {
            const error = await response.text();
            showNotification(`Failed to save: ${error}`, 'error');
            return false;
        }
    } catch (error) {
        showNotification('Failed to save configuration', 'error');
        console.error(error);
        return false;
    }
}

// Refresh status
async function refreshStatus() {
    try {
        const response = await fetch('/api/status');
        const status = await response.json();

        // Update banner
        const banner = document.getElementById('status-banner');
        if (status.is_safe) {
            banner.className = 'status-banner status-safe';
            banner.textContent = '✓ SAFE - All monitors reporting safe conditions';
        } else {
            banner.className = 'status-banner status-unsafe';
            banner.textContent = '⚠ UNSAFE - One or more monitors reporting unsafe conditions';
        }

        // Update status list
        renderMonitorStatus(status.monitors);
    } catch (error) {
        console.error('Failed to refresh status:', error);
    }
}

// Render all
function renderAll() {
    renderMqttServers();
    renderMqttMonitors();
    renderAlpacaMonitors();
    renderSettings();
}

// Render monitor status
function renderMonitorStatus(monitors) {
    const container = document.getElementById('monitor-status-list');

    if (!monitors || monitors.length === 0) {
        container.innerHTML = '<p style="text-align: center; color: #999;">No monitors configured</p>';
        return;
    }

    container.innerHTML = monitors.map(monitor => {
        const statusClass = monitor.enabled ? (monitor.is_safe ? 'safe' : 'unsafe') : 'disabled';
        const statusText = monitor.enabled ? (monitor.is_safe ? 'SAFE' : 'UNSAFE') : 'DISABLED';
        const lastUpdate = monitor.last_update
            ? new Date(monitor.last_update).toLocaleString()
            : 'Never';

        // Calculate pending state info
        let pendingInfo = '';
        if (monitor.pending_is_safe !== null && monitor.pending_is_safe !== undefined && monitor.pending_since) {
            const pendingSince = new Date(monitor.pending_since);
            const now = new Date();
            const elapsedSeconds = Math.floor((now - pendingSince) / 1000);
            const remainingSeconds = Math.max(0, monitor.hold_time_seconds - elapsedSeconds);

            const currentStateText = monitor.is_safe ? 'SAFE' : 'UNSAFE';
            const pendingStateText = monitor.pending_is_safe ? 'SAFE' : 'UNSAFE';
            const pendingClass = monitor.pending_is_safe ? 'safe' : 'unsafe';

            pendingInfo = `
                <div class="card-row" style="grid-column: 1 / -1;">
                    <div style="background-color: #fff3cd; border: 1px solid #ffc107; padding: 8px; border-radius: 4px;">
                        <strong>⏱ Pending Change:</strong> ${currentStateText} → <span class="${pendingClass}">${pendingStateText}</span>
                        <br>
                        <span style="font-size: 0.9em;">Countdown: ${remainingSeconds}s remaining (hold time: ${monitor.hold_time_seconds}s)</span>
                    </div>
                </div>
            `;
        }

        return `
            <div class="card ${statusClass}">
                <h3>
                    <span class="status-indicator ${statusClass}"></span>
                    ${monitor.name} (${monitor.monitor_type})
                    <label class="monitor-toggle" style="float: right;">
                        <input type="checkbox" ${monitor.enabled ? 'checked' : ''} onchange="toggleMonitor('${monitor.id}', '${monitor.monitor_type}', this.checked)">
                        <span>Enabled</span>
                    </label>
                </h3>
                <div class="card-grid">
                    <div class="card-row">
                        <span class="card-label">Status:</span>
                        <span>${statusText}</span>
                    </div>
                    ${pendingInfo}
                    <div class="card-row">
                        <span class="card-label">Current Value:</span>
                        <span>${monitor.current_value !== null ? monitor.current_value.toFixed(2) : 'N/A'}</span>
                    </div>
                    <div class="card-row">
                        <span class="card-label">Threshold:</span>
                        <span>${monitor.threshold.toFixed(2)}</span>
                    </div>
                    <div class="card-row">
                        <span class="card-label">Last Update:</span>
                        <span>${lastUpdate}</span>
                    </div>
                    ${monitor.hold_time_seconds > 0 ? `
                    <div class="card-row">
                        <span class="card-label">Hold Time:</span>
                        <span>${monitor.hold_time_seconds}s</span>
                    </div>
                    ` : ''}
                    ${monitor.error ? `
                    <div class="card-row">
                        <span class="card-label">Error:</span>
                        <span style="color: #f44336;">${monitor.error}</span>
                    </div>
                    ` : ''}
                    ${monitor.raw_payload ? `
                    <div class="card-row" style="grid-column: 1 / -1;">
                        <span class="card-label">Raw Payload:</span>
                        <span style="font-family: monospace; font-size: 0.85em; word-break: break-all;">${monitor.raw_payload}</span>
                    </div>
                    ` : ''}
                </div>
            </div>
        `;
    }).join('');
}

// Toggle monitor enabled/disabled
async function toggleMonitor(id, monitorType, enabled) {
    const endpoint = monitorType === 'MQTT'
        ? `/api/monitors/mqtt/${id}/toggle`
        : `/api/monitors/alpaca/${id}/toggle`;

    try {
        const response = await fetch(endpoint, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ enabled })
        });

        if (response.ok) {
            showNotification(`Monitor ${enabled ? 'enabled' : 'disabled'} successfully`, 'success');
            await loadConfig();  // Reload config to update UI
            await refreshStatus(); // Refresh status immediately
        } else {
            const error = await response.text();
            showNotification(`Failed to toggle monitor: ${error}`, 'error');
        }
    } catch (error) {
        showNotification('Failed to toggle monitor', 'error');
        console.error(error);
    }
}

// MQTT Servers
function renderMqttServers() {
    const container = document.getElementById('mqtt-servers-list');
    const servers = Object.values(config.mqtt_servers);

    if (servers.length === 0) {
        container.innerHTML = '<p style="text-align: center; color: #999;">No MQTT servers configured</p>';
        return;
    }

    container.innerHTML = servers.map(server => `
        <div class="list-item">
            <div class="list-item-info">
                <strong>${server.id}</strong> - ${server.host}:${server.port}
                ${server.username ? ` (user: ${server.username})` : ''}
            </div>
            <div class="list-item-actions">
                <button class="btn btn-primary" onclick="editMqttServer('${server.id}')">Edit</button>
                <button class="btn btn-danger" onclick="deleteMqttServer('${server.id}')">Delete</button>
            </div>
        </div>
    `).join('');
}

function showAddMqttServer() {
    currentEditId = null;
    currentEditType = 'mqtt-server';
    document.getElementById('mqtt-server-form').style.display = 'block';
    document.getElementById('mqtt-server-id').value = '';
    document.getElementById('mqtt-server-id').disabled = false;
    document.getElementById('mqtt-server-host').value = '';
    document.getElementById('mqtt-server-port').value = '1883';
    document.getElementById('mqtt-server-username').value = '';
    document.getElementById('mqtt-server-password').value = '';
}

function editMqttServer(id) {
    const server = config.mqtt_servers[id];
    if (!server) return;

    currentEditId = id;
    currentEditType = 'mqtt-server';
    document.getElementById('mqtt-server-form').style.display = 'block';
    document.getElementById('mqtt-server-id').value = server.id;
    document.getElementById('mqtt-server-id').disabled = true; // Can't change ID when editing
    document.getElementById('mqtt-server-host').value = server.host;
    document.getElementById('mqtt-server-port').value = server.port;
    document.getElementById('mqtt-server-username').value = server.username || '';
    document.getElementById('mqtt-server-password').value = server.password || '';
}

function cancelMqttServer() {
    document.getElementById('mqtt-server-form').style.display = 'none';
    currentEditId = null;
    currentEditType = null;
}

function saveMqttServer() {
    const id = document.getElementById('mqtt-server-id').value.trim();
    const host = document.getElementById('mqtt-server-host').value.trim();
    const port = parseInt(document.getElementById('mqtt-server-port').value);
    const username = document.getElementById('mqtt-server-username').value.trim();
    const password = document.getElementById('mqtt-server-password').value.trim();

    if (!id || !host || !port) {
        showNotification('Please fill in all required fields', 'error');
        return;
    }

    // Check for duplicate ID when adding new
    if (!currentEditId && config.mqtt_servers[id]) {
        showNotification('Server ID already exists', 'error');
        return;
    }

    config.mqtt_servers[id] = {
        id,
        host,
        port,
        username: username || null,
        password: password || null
    };

    saveConfig().then(success => {
        if (success) {
            cancelMqttServer();
            renderMqttServers();
            updateMqttServerSelects();
        }
    });
}

function deleteMqttServer(id) {
    if (!confirm(`Delete MQTT server '${id}'?`)) return;

    // Check if any monitors use this server
    const monitors = Object.values(config.mqtt_monitors).filter(m => m.server_id === id);
    if (monitors.length > 0) {
        showNotification(`Cannot delete: ${monitors.length} monitor(s) use this server`, 'error');
        return;
    }

    delete config.mqtt_servers[id];
    saveConfig().then(success => {
        if (success) {
            renderMqttServers();
        }
    });
}

// MQTT Monitors
function renderMqttMonitors() {
    const container = document.getElementById('mqtt-monitors-list');
    const monitors = Object.values(config.mqtt_monitors);

    if (monitors.length === 0) {
        container.innerHTML = '<p style="text-align: center; color: #999;">No MQTT monitors configured</p>';
        return;
    }

    container.innerHTML = monitors.map(monitor => `
        <div class="list-item">
            <div class="list-item-info">
                <strong>${monitor.name}</strong> (${monitor.id})<br>
                Server: ${monitor.server_id}, Topic: ${monitor.topic}<br>
                JSON Path: ${monitor.json_path || '(raw value)'}, Threshold: ${getOperatorSymbol(monitor.operator)} ${monitor.threshold}
            </div>
            <div class="list-item-actions">
                <button class="btn btn-primary" onclick="editMqttMonitor('${monitor.id}')">Edit</button>
                <button class="btn btn-danger" onclick="deleteMqttMonitor('${monitor.id}')">Delete</button>
            </div>
        </div>
    `).join('');
}

function updateMqttServerSelects() {
    const select = document.getElementById('mqtt-monitor-server');
    const servers = Object.values(config.mqtt_servers);

    select.innerHTML = servers.map(server =>
        `<option value="${server.id}">${server.id} (${server.host})</option>`
    ).join('');
}

function showAddMqttMonitor() {
    updateMqttServerSelects();
    currentEditId = null;
    currentEditType = 'mqtt-monitor';
    document.getElementById('mqtt-monitor-form').style.display = 'block';
    document.getElementById('mqtt-monitor-id').value = '';
    document.getElementById('mqtt-monitor-id').disabled = false;
    document.getElementById('mqtt-monitor-name').value = '';
    document.getElementById('mqtt-monitor-topic').value = '';
    document.getElementById('mqtt-monitor-json-path').value = '';
    document.getElementById('mqtt-monitor-threshold').value = '';
    document.getElementById('mqtt-monitor-operator').value = 'greaterthan';
    document.getElementById('mqtt-monitor-safe-when-true').checked = true;
    document.getElementById('mqtt-monitor-timeout').value = '300';
    document.getElementById('mqtt-monitor-hold-time').value = '0';
}

function editMqttMonitor(id) {
    const monitor = config.mqtt_monitors[id];
    if (!monitor) return;

    updateMqttServerSelects();
    currentEditId = id;
    currentEditType = 'mqtt-monitor';
    document.getElementById('mqtt-monitor-form').style.display = 'block';
    document.getElementById('mqtt-monitor-id').value = monitor.id;
    document.getElementById('mqtt-monitor-id').disabled = true; // Can't change ID when editing
    document.getElementById('mqtt-monitor-name').value = monitor.name;
    document.getElementById('mqtt-monitor-server').value = monitor.server_id;
    document.getElementById('mqtt-monitor-topic').value = monitor.topic;
    document.getElementById('mqtt-monitor-json-path').value = monitor.json_path || '';
    document.getElementById('mqtt-monitor-threshold').value = monitor.threshold;
    document.getElementById('mqtt-monitor-operator').value = monitor.operator;
    document.getElementById('mqtt-monitor-safe-when-true').checked = monitor.safe_when_true;
    document.getElementById('mqtt-monitor-timeout').value = monitor.timeout_seconds || 300;
    document.getElementById('mqtt-monitor-hold-time').value = monitor.hold_time_seconds || 0;
}

function cancelMqttMonitor() {
    document.getElementById('mqtt-monitor-form').style.display = 'none';
    currentEditId = null;
    currentEditType = null;
}

function saveMqttMonitor() {
    const id = document.getElementById('mqtt-monitor-id').value.trim();
    const name = document.getElementById('mqtt-monitor-name').value.trim();
    const server_id = document.getElementById('mqtt-monitor-server').value;
    const topic = document.getElementById('mqtt-monitor-topic').value.trim();
    const json_path_raw = document.getElementById('mqtt-monitor-json-path').value.trim();
    const threshold = parseFloat(document.getElementById('mqtt-monitor-threshold').value);
    const operator = document.getElementById('mqtt-monitor-operator').value;
    const safe_when_true = document.getElementById('mqtt-monitor-safe-when-true').checked;
    const timeout_seconds = parseInt(document.getElementById('mqtt-monitor-timeout').value);
    const hold_time_seconds = parseInt(document.getElementById('mqtt-monitor-hold-time').value);

    if (!id || !name || !server_id || !topic || isNaN(threshold) || isNaN(timeout_seconds) || timeout_seconds < 0 || isNaN(hold_time_seconds) || hold_time_seconds < 0) {
        showNotification('Please fill in all required fields', 'error');
        return;
    }

    // Check for duplicate ID when adding new
    if (!currentEditId && config.mqtt_monitors[id]) {
        showNotification('Monitor ID already exists', 'error');
        return;
    }

    config.mqtt_monitors[id] = {
        id,
        name,
        server_id,
        topic,
        json_path: json_path_raw || null, // null for raw values, string for JSON path
        threshold,
        operator,
        safe_when_true,
        timeout_seconds,
        hold_time_seconds,
        enabled: true // New monitors are enabled by default
    };

    saveConfig().then(success => {
        if (success) {
            cancelMqttMonitor();
            renderMqttMonitors();
        }
    });
}

function deleteMqttMonitor(id) {
    if (!confirm(`Delete MQTT monitor '${id}'?`)) return;

    delete config.mqtt_monitors[id];
    saveConfig().then(success => {
        if (success) {
            renderMqttMonitors();
        }
    });
}

// Alpaca Monitors
function renderAlpacaMonitors() {
    const container = document.getElementById('alpaca-monitors-list');
    const monitors = Object.values(config.alpaca_monitors);

    if (monitors.length === 0) {
        container.innerHTML = '<p style="text-align: center; color: #999;">No Alpaca monitors configured</p>';
        return;
    }

    container.innerHTML = monitors.map(monitor => `
        <div class="list-item">
            <div class="list-item-info">
                <strong>${monitor.name}</strong> (${monitor.id})<br>
                Host: ${monitor.host}:${monitor.port}, Device: ${monitor.device_type}/${monitor.device_number}<br>
                Property: ${monitor.property}, Threshold: ${getOperatorSymbol(monitor.operator)} ${monitor.threshold}
            </div>
            <div class="list-item-actions">
                <button class="btn btn-primary" onclick="editAlpacaMonitor('${monitor.id}')">Edit</button>
                <button class="btn btn-danger" onclick="deleteAlpacaMonitor('${monitor.id}')">Delete</button>
            </div>
        </div>
    `).join('');
}

function showAddAlpacaMonitor() {
    currentEditId = null;
    currentEditType = 'alpaca-monitor';
    document.getElementById('alpaca-monitor-form').style.display = 'block';
    document.getElementById('alpaca-monitor-id').value = '';
    document.getElementById('alpaca-monitor-id').disabled = false;
    document.getElementById('alpaca-monitor-name').value = '';
    document.getElementById('alpaca-monitor-host').value = '';
    document.getElementById('alpaca-monitor-port').value = '11111';
    document.getElementById('alpaca-monitor-device-type').value = '';
    document.getElementById('alpaca-monitor-device-number').value = '0';
    document.getElementById('alpaca-monitor-property').value = '';
    document.getElementById('alpaca-monitor-threshold').value = '';
    document.getElementById('alpaca-monitor-operator').value = 'greaterthan';
    document.getElementById('alpaca-monitor-safe-when-true').checked = true;
    document.getElementById('alpaca-monitor-timeout').value = '300';
    document.getElementById('alpaca-monitor-hold-time').value = '0';
}

function editAlpacaMonitor(id) {
    const monitor = config.alpaca_monitors[id];
    if (!monitor) return;

    currentEditId = id;
    currentEditType = 'alpaca-monitor';
    document.getElementById('alpaca-monitor-form').style.display = 'block';
    document.getElementById('alpaca-monitor-id').value = monitor.id;
    document.getElementById('alpaca-monitor-id').disabled = true;
    document.getElementById('alpaca-monitor-name').value = monitor.name;
    document.getElementById('alpaca-monitor-host').value = monitor.host;
    document.getElementById('alpaca-monitor-port').value = monitor.port;
    document.getElementById('alpaca-monitor-device-type').value = monitor.device_type;
    document.getElementById('alpaca-monitor-device-number').value = monitor.device_number;
    document.getElementById('alpaca-monitor-property').value = monitor.property;
    document.getElementById('alpaca-monitor-threshold').value = monitor.threshold;
    document.getElementById('alpaca-monitor-operator').value = monitor.operator;
    document.getElementById('alpaca-monitor-safe-when-true').checked = monitor.safe_when_true;
    document.getElementById('alpaca-monitor-timeout').value = monitor.timeout_seconds || 300;
    document.getElementById('alpaca-monitor-hold-time').value = monitor.hold_time_seconds || 0;
}

function cancelAlpacaMonitor() {
    document.getElementById('alpaca-monitor-form').style.display = 'none';
    currentEditId = null;
    currentEditType = null;
}

function saveAlpacaMonitor() {
    const id = document.getElementById('alpaca-monitor-id').value.trim();
    const name = document.getElementById('alpaca-monitor-name').value.trim();
    const host = document.getElementById('alpaca-monitor-host').value.trim();
    const port = parseInt(document.getElementById('alpaca-monitor-port').value);
    const device_type = document.getElementById('alpaca-monitor-device-type').value.trim();
    const device_number = parseInt(document.getElementById('alpaca-monitor-device-number').value);
    const property = document.getElementById('alpaca-monitor-property').value.trim();
    const threshold = parseFloat(document.getElementById('alpaca-monitor-threshold').value);
    const operator = document.getElementById('alpaca-monitor-operator').value;
    const safe_when_true = document.getElementById('alpaca-monitor-safe-when-true').checked;
    const timeout_seconds = parseInt(document.getElementById('alpaca-monitor-timeout').value);
    const hold_time_seconds = parseInt(document.getElementById('alpaca-monitor-hold-time').value);

    if (!id || !name || !host || !port || !device_type || isNaN(device_number) || !property || isNaN(threshold) || isNaN(timeout_seconds) || timeout_seconds < 0 || isNaN(hold_time_seconds) || hold_time_seconds < 0) {
        showNotification('Please fill in all required fields', 'error');
        return;
    }

    // Check for duplicate ID when adding new
    if (!currentEditId && config.alpaca_monitors[id]) {
        showNotification('Monitor ID already exists', 'error');
        return;
    }

    config.alpaca_monitors[id] = {
        id,
        name,
        host,
        port,
        device_type,
        device_number,
        property,
        threshold,
        operator,
        safe_when_true,
        timeout_seconds,
        hold_time_seconds,
        enabled: true // New monitors are enabled by default
    };

    saveConfig().then(success => {
        if (success) {
            cancelAlpacaMonitor();
            renderAlpacaMonitors();
        }
    });
}

function deleteAlpacaMonitor(id) {
    if (!confirm(`Delete Alpaca monitor '${id}'?`)) return;

    delete config.alpaca_monitors[id];
    saveConfig().then(success => {
        if (success) {
            renderAlpacaMonitors();
        }
    });
}

// Settings
function renderSettings() {
    document.getElementById('device-name').value = config.device_name || 'LLAMA Safety Monitor';
    document.getElementById('server-port').value = config.server_port || 8080;
}

function saveSettings() {
    config.device_name = document.getElementById('device-name').value.trim();
    config.server_port = parseInt(document.getElementById('server-port').value);

    saveConfig().then(success => {
        if (success) {
            showNotification('Settings saved. Note: Server port changes require restart.', 'success');
        }
    });
}

// Utilities
function getOperatorSymbol(operator) {
    switch (operator) {
        case 'greaterthan': return '>';
        case 'lessthan': return '<';
        case 'equal': return '=';
        default: return operator;
    }
}

function showNotification(message, type) {
    const notification = document.createElement('div');
    notification.className = `notification ${type}`;
    notification.textContent = message;
    document.body.appendChild(notification);

    setTimeout(() => {
        notification.remove();
    }, 3000);
}
