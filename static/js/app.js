// Global state
let config = {
    mqtt_servers: {},
    mqtt_monitors: {},
    alpaca_monitors: {},
    monitor_groups: {},
    switches: {},
    switch_device: {
        enabled: true,
        name: null,
        description: null,
        auto_connect: true
    },
    server_port: 8080,
    device_name: "LLAMA Safety Monitor",
    location: "",
    logging_enabled: false
};

let loggingStatus = {
    enabled: false,
    current_file: "",
    files: []
};

let currentEditId = null;
let currentEditType = null;

// HTML escape function to prevent XSS
function escapeHtml(text) {
    if (!text) return '';
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

// Initialize
document.addEventListener('DOMContentLoaded', async () => {
    await loadConfig();
    await refreshStatus();
    await loadLoggingStatus();

    // Auto-connect ASCOM devices on page load
    await autoConnectDevices();

    // Auto-refresh status every 5 seconds
    setInterval(refreshStatus, 5000);

    // Handle hash navigation (for setup redirects)
    if (window.location.hash) {
        const tabName = window.location.hash.substring(1);
        const validTabs = ['status', 'mqtt-servers', 'mqtt-monitors', 'alpaca-monitors', 'switches', 'weather-devices', 'settings'];
        if (validTabs.includes(tabName)) {
            switchTab(tabName);
        }
    }
});

// Tab switching
function switchTab(tabName) {
    // Update tab buttons
    document.querySelectorAll('.tab').forEach(tab => {
        tab.classList.remove('active');
        // Find the button for this tab by checking its onclick attribute
        if (tab.getAttribute('onclick') === `switchTab('${tabName}')`) {
            tab.classList.add('active');
        }
    });

    // Update tab content
    document.querySelectorAll('.tab-content').forEach(content => {
        content.classList.remove('active');
    });
    document.getElementById(`tab-${tabName}`).classList.add('active');

    // Update URL hash without triggering navigation
    history.replaceState(null, null, `#${tabName}`);
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
        if (!status.is_connected) {
            banner.className = 'status-banner status-disconnected';
            banner.innerHTML = '⚡ DISCONNECTED - ASCOM device not connected. <span style="font-size: 0.85em; opacity: 0.9;">Clients must call PUT /connected to enable.</span>';
        } else if (status.is_safe) {
            banner.className = 'status-banner status-safe';
            banner.textContent = '✓ SAFE - All monitors reporting safe conditions';
        } else {
            banner.className = 'status-banner status-unsafe';
            banner.textContent = '⚠ UNSAFE - One or more monitors reporting unsafe conditions';
        }

        // Update connection status indicator
        const connectionStatus = document.getElementById('connection-status');
        const connectionText = connectionStatus.querySelector('.connection-text');
        const connectionBtn = document.getElementById('connection-toggle-btn');
        if (status.is_connected) {
            connectionStatus.className = 'connection-status connected';
            connectionText.textContent = 'Connected - ASCOM clients can query safety status';
            connectionBtn.textContent = 'Disconnect';
            connectionBtn.className = 'btn btn-danger connection-toggle';
        } else {
            connectionStatus.className = 'connection-status disconnected';
            connectionText.textContent = 'Disconnected - Click Connect to enable ASCOM queries';
            connectionBtn.textContent = 'Connect';
            connectionBtn.className = 'btn btn-success connection-toggle';
        }

        // Update status list
        renderMonitorStatus(status.monitors);

        // Fetch and render group status
        try {
            const groupsResponse = await fetch('/api/groups/status');
            const groupStatuses = await groupsResponse.json();
            renderGroupStatus(groupStatuses);
        } catch (err) {
            console.warn('Failed to fetch group status:', err);
        }

        // Update switch status list
        renderSwitchStatus(status.switches || []);

        // Fetch and render weather status
        try {
            const weatherResponse = await fetch('/api/weather/status');
            const weatherStatus = await weatherResponse.json();
            renderWeatherStatus(weatherStatus);
        } catch (err) {
            console.error('Failed to fetch weather status:', err);
        }
    } catch (error) {
        console.error('Failed to refresh status:', error);
    }
}

// Render weather status
function renderWeatherStatus(weatherDevices) {
    const container = document.getElementById('weather-status-list');

    if (!weatherDevices || weatherDevices.length === 0) {
        container.innerHTML = '<p style="text-align: center; color: #999;">No weather devices configured</p>';
        return;
    }

    container.innerHTML = weatherDevices.map(device => {
        const statusClass = device.enabled ? (device.is_safe ? 'safe' : 'unsafe') : 'disabled';
        const statusText = device.enabled ? (device.is_safe ? 'SAFE' : 'UNSAFE') : 'DISABLED';
        const lastUpdate = device.last_update
            ? new Date(device.last_update).toLocaleString()
            : 'Never';

        // Build measurements table
        const measurements = Object.entries(device.measurements || {}).map(([key, m]) => {
            let measurementClass = '';
            let statusIndicator = '';

            if (m.is_monitored) {
                measurementClass = m.is_safe ? 'measurement-safe' : 'measurement-unsafe';
                statusIndicator = m.is_safe
                    ? '<span style="color: #27ae60; font-weight: bold;">✓</span>'
                    : '<span style="color: #e74c3c; font-weight: bold;">✗</span>';
            } else {
                measurementClass = 'measurement-unmonitored';
                statusIndicator = '<span style="color: #95a5a6;">—</span>';
            }

            const thresholdDisplay = m.is_monitored ? m.threshold.toFixed(2) : '—';

            return `
                <tr class="${measurementClass}">
                    <td>${m.name}</td>
                    <td style="text-align: right;">${m.value.toFixed(2)}</td>
                    <td style="text-align: right;">${thresholdDisplay}</td>
                    <td style="text-align: center;">${statusIndicator}</td>
                </tr>
            `;
        }).join('');

        return `
            <div class="status-card ${statusClass}">
                <div class="status-header">
                    <span class="status-name">${device.name}</span>
                    <span class="status-badge ${statusClass}">${statusText}</span>
                </div>
                <div class="status-details">
                    <div><strong>Device:</strong> Weather Device ${device.device_number}</div>
                    <div><strong>Last Update:</strong> ${lastUpdate}</div>
                    ${device.error ? `<div style="color: #e74c3c;"><strong>Error:</strong> ${device.error}</div>` : ''}
                </div>
                <table class="measurements-table" style="width: 100%; margin-top: 10px; border-collapse: collapse;">
                    <thead>
                        <tr style="border-bottom: 1px solid #ddd;">
                            <th style="text-align: left; padding: 5px;">Measurement</th>
                            <th style="text-align: right; padding: 5px;">Value</th>
                            <th style="text-align: right; padding: 5px;">Threshold</th>
                            <th style="text-align: center; padding: 5px;">Status</th>
                        </tr>
                    </thead>
                    <tbody>
                        ${measurements}
                    </tbody>
                </table>
            </div>
        `;
    }).join('');
}

// Render all
function renderAll() {
    renderMqttServers();
    renderMqttMonitors();
    renderAlpacaMonitors();
    renderMonitorGroups();
    renderSwitches();
    renderSettings();
    renderSwitchDeviceSettings();
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
                        <span style="color: #f44336;">${escapeHtml(monitor.error)}</span>
                    </div>
                    ` : ''}
                    ${monitor.raw_payload ? `
                    <div class="card-row" style="grid-column: 1 / -1;">
                        <span class="card-label">Raw Payload:</span>
                        <span style="font-family: monospace; font-size: 0.85em; word-break: break-all;">${escapeHtml(monitor.raw_payload)}</span>
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
    // Switch integration fields
    document.getElementById('mqtt-monitor-include-in-safety').checked = true;
    document.getElementById('mqtt-monitor-include-in-switch').checked = false;
    document.getElementById('mqtt-monitor-switch-name').value = '';
    document.getElementById('mqtt-monitor-switch-timeout').value = '';
    document.getElementById('mqtt-monitor-switch-hold-time').value = '';
    document.getElementById('mqtt-switch-fields').style.display = 'none';
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
    const includeInSafety = monitor.include_in_safety !== undefined ? monitor.include_in_safety : true;
    document.getElementById('mqtt-monitor-include-in-safety').checked = includeInSafety;
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
    const include_in_safety = document.getElementById('mqtt-monitor-include-in-safety').checked;

    if (!id || !name || !server_id || !topic || isNaN(threshold) || isNaN(timeout_seconds) || isNaN(hold_time_seconds) || hold_time_seconds < 0) {
        showNotification('Please fill in all required fields', 'error');
        return;
    }

    // Check for duplicate ID when adding new
    if (!currentEditId && config.mqtt_monitors[id]) {
        showNotification('Monitor ID already exists', 'error');
        return;
    }

    // Preserve enabled state when editing
    const existingEnabled = currentEditId && config.mqtt_monitors[currentEditId]
        ? config.mqtt_monitors[currentEditId].enabled
        : true;

    config.mqtt_monitors[id] = {
        id,
        name,
        server_id,
        topic,
        json_path: json_path_raw || null,
        threshold,
        operator,
        safe_when_true,
        timeout_seconds,
        hold_time_seconds,
        enabled: existingEnabled,
        include_in_safety
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
    document.getElementById('alpaca-monitor-include-in-safety').checked = true;
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
    const includeInSafety = monitor.include_in_safety !== undefined ? monitor.include_in_safety : true;
    document.getElementById('alpaca-monitor-include-in-safety').checked = includeInSafety;
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
    const include_in_safety = document.getElementById('alpaca-monitor-include-in-safety').checked;

    if (!id || !name || !host || !port || !device_type || isNaN(device_number) || !property || isNaN(threshold) || isNaN(timeout_seconds) || isNaN(hold_time_seconds) || hold_time_seconds < 0) {
        showNotification('Please fill in all required fields', 'error');
        return;
    }

    // Check for duplicate ID when adding new
    if (!currentEditId && config.alpaca_monitors[id]) {
        showNotification('Monitor ID already exists', 'error');
        return;
    }

    // Preserve enabled state when editing
    const existingEnabled = currentEditId && config.alpaca_monitors[currentEditId]
        ? config.alpaca_monitors[currentEditId].enabled
        : true;

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
        enabled: existingEnabled,
        include_in_safety
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
    document.getElementById('device-location').value = config.location || '';
    document.getElementById('server-port').value = config.server_port || 8080;
}

function saveSettings() {
    config.device_name = document.getElementById('device-name').value.trim();
    config.location = document.getElementById('device-location').value.trim();
    config.server_port = parseInt(document.getElementById('server-port').value);

    saveConfig().then(success => {
        if (success) {
            showNotification('Settings saved. Note: Server port and location changes require restart.', 'success');
        }
    });
}

// Switch Device Settings
function renderSwitchDeviceSettings() {
    const switchDevice = config.switch_device || {
        enabled: true,
        name: null,
        description: null,
        auto_connect: true
    };

    document.getElementById('switch-device-enabled').checked = switchDevice.enabled !== false;
    document.getElementById('switch-device-name').value = switchDevice.name || '';
    document.getElementById('switch-device-description').value = switchDevice.description || '';
    document.getElementById('switch-device-auto-connect').checked = switchDevice.auto_connect !== false;
}

function saveSwitchDeviceSettings() {
    const enabled = document.getElementById('switch-device-enabled').checked;
    const name = document.getElementById('switch-device-name').value.trim();
    const description = document.getElementById('switch-device-description').value.trim();
    const auto_connect = document.getElementById('switch-device-auto-connect').checked;

    config.switch_device = {
        enabled,
        name: name || null,
        description: description || null,
        auto_connect
    };

    saveConfig().then(success => {
        if (success) {
            showNotification('Switch device settings saved. Changes take effect after reload.', 'success');
        }
    });
}

// Logging
async function loadLoggingStatus() {
    try {
        const response = await fetch('/api/logging/status');
        loggingStatus = await response.json();
        renderLoggingStatus();
    } catch (error) {
        console.error('Failed to load logging status:', error);
    }
}

function renderLoggingStatus() {
    document.getElementById('logging-enabled').checked = loggingStatus.enabled;

    const container = document.getElementById('log-files-list');

    if (loggingStatus.files.length === 0) {
        container.innerHTML = '<p style="color: #999; font-size: 0.9em;">No log files available</p>';
        return;
    }

    container.innerHTML = `
        <h4 style="margin: 0 0 10px 0; font-size: 0.95em;">Available Log Files:</h4>
        <div class="list-container" style="max-height: 200px; overflow-y: auto;">
            ${loggingStatus.files.map(file => {
                const sizeKB = (file.size / 1024).toFixed(1);
                const modified = file.modified ? new Date(file.modified).toLocaleString() : 'Unknown';
                const isCurrent = file.filename === loggingStatus.current_file;
                return `
                    <div class="list-item" style="padding: 8px;">
                        <div class="list-item-info" style="font-size: 0.85em;">
                            <strong>${escapeHtml(file.filename)}</strong>${isCurrent ? ' (current)' : ''}<br>
                            Size: ${sizeKB} KB, Modified: ${modified}
                        </div>
                        <div class="list-item-actions">
                            <button class="btn btn-primary" style="font-size: 0.8em; padding: 4px 8px;" onclick="downloadLogFile('${escapeHtml(file.filename)}')">Download</button>
                        </div>
                    </div>
                `;
            }).join('')}
        </div>
    `;
}

async function toggleLogging(enabled) {
    try {
        const response = await fetch('/api/logging/toggle', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ enabled })
        });

        if (response.ok) {
            showNotification(`Logging ${enabled ? 'enabled' : 'disabled'}`, 'success');
            await loadLoggingStatus();
        } else {
            const error = await response.text();
            showNotification(`Failed to toggle logging: ${error}`, 'error');
            // Revert checkbox
            document.getElementById('logging-enabled').checked = !enabled;
        }
    } catch (error) {
        showNotification('Failed to toggle logging', 'error');
        console.error(error);
        // Revert checkbox
        document.getElementById('logging-enabled').checked = !enabled;
    }
}

function downloadCurrentLog() {
    window.location.href = '/api/logging/download';
}

function downloadLogFile(filename) {
    window.location.href = `/api/logging/download/${encodeURIComponent(filename)}`;
}

// Auto-connect ASCOM devices on page load
async function autoConnectDevices() {
    try {
        // Connect Safety Monitor
        await fetch('/api/v1/safetymonitor/0/connected', {
            method: 'PUT',
            headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
            body: 'Connected=true'
        });

        // Connect all weather devices with auto_connect enabled
        if (config.weather_devices) {
            for (const [deviceNumber, deviceConfig] of Object.entries(config.weather_devices)) {
                if (deviceConfig.auto_connect && deviceConfig.enabled) {
                    await fetch(`/api/v1/observingconditions/${deviceNumber}/connected`, {
                        method: 'PUT',
                        headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
                        body: 'Connected=true'
                    });
                }
            }
        }

        // Connect switch device if auto_connect enabled
        if (config.switch_device && config.switch_device.enabled && config.switch_device.auto_connect) {
            await fetch('/api/v1/switch/0/connected', {
                method: 'PUT',
                headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
                body: 'Connected=true'
            });
        }

        console.log('Auto-connected ASCOM devices');
    } catch (error) {
        console.error('Failed to auto-connect devices:', error);
    }
}

// Toggle ASCOM connection state
async function toggleConnection() {
    try {
        // Get current state from the button text
        const connectionBtn = document.getElementById('connection-toggle-btn');
        const isCurrentlyConnected = connectionBtn.textContent === 'Disconnect';
        const newState = !isCurrentlyConnected;

        const response = await fetch('/api/v1/safetymonitor/0/connected', {
            method: 'PUT',
            headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
            body: `Connected=${newState}`
        });

        if (response.ok) {
            showNotification(`ASCOM device ${newState ? 'connected' : 'disconnected'}`, 'success');
            await refreshStatus();
        } else {
            const error = await response.text();
            showNotification(`Failed to toggle connection: ${error}`, 'error');
        }
    } catch (error) {
        showNotification('Failed to toggle connection', 'error');
        console.error(error);
    }
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

// ============================================================================
// Switch Status Display
// ============================================================================

function renderSwitchStatus(switches) {
    const container = document.getElementById('switches-status-list');
    const section = document.getElementById('switches-status-section');

    if (!switches || switches.length === 0) {
        section.style.display = 'none';
        return;
    }

    section.style.display = 'block';

    container.innerHTML = switches.map(sw => {
        const statusClass = sw.enabled ? (sw.is_triggered ? 'unsafe' : 'safe') : 'disabled';
        const statusText = sw.enabled ? (sw.is_triggered ? 'TRIGGERED' : 'NOT TRIGGERED') : 'DISABLED';
        const lastUpdate = sw.last_update
            ? new Date(sw.last_update).toLocaleString()
            : 'Never';

        // Format logic description
        let logicText = '';
        if (sw.logic === 'any') {
            logicText = 'ANY source triggered (OR)';
        } else if (sw.logic === 'all') {
            logicText = 'ALL sources triggered (AND)';
        } else if (sw.logic && sw.logic.min_of) {
            logicText = `At least ${sw.logic.min_of.count} of ${sw.source_states.length} sources`;
        }

        // Calculate pending state info
        let pendingInfo = '';
        if (sw.pending_is_triggered !== null && sw.pending_is_triggered !== undefined && sw.pending_since) {
            const pendingSince = new Date(sw.pending_since);
            const now = new Date();
            const elapsedSeconds = Math.floor((now - pendingSince) / 1000);
            const remainingSeconds = Math.max(0, sw.hold_time_seconds - elapsedSeconds);

            const currentStateText = sw.is_triggered ? 'TRIGGERED' : 'NOT TRIGGERED';
            const pendingStateText = sw.pending_is_triggered ? 'TRIGGERED' : 'NOT TRIGGERED';
            const pendingClass = sw.pending_is_triggered ? 'unsafe' : 'safe';

            pendingInfo = `
                <div class="card-row" style="grid-column: 1 / -1;">
                    <div style="background-color: #fff3cd; border: 1px solid #ffc107; padding: 8px; border-radius: 4px;">
                        <strong>⏱ Pending Change:</strong> ${currentStateText} → <span class="${pendingClass}">${pendingStateText}</span>
                        <br>
                        <span style="font-size: 0.9em;">Countdown: ${remainingSeconds}s remaining (hold time: ${sw.hold_time_seconds}s)</span>
                    </div>
                </div>
            `;
        }

        // Format source states
        const sourcesHtml = sw.source_states.map(source => {
            const sourceClass = source.is_triggered ? 'unsafe' : 'safe';
            const sourceIcon = source.is_triggered ? '⚠' : '✓';
            return `
                <div style="display: flex; align-items: center; gap: 8px; padding: 4px 0;">
                    <span class="status-indicator ${sourceClass}" style="width: 10px; height: 10px;"></span>
                    <span>${sourceIcon} ${escapeHtml(source.monitor_name)}</span>
                    ${source.current_value !== null ? `<span style="color: #666;">(${source.current_value.toFixed(2)})</span>` : ''}
                    ${source.error ? `<span style="color: #f44336; font-size: 0.85em;">[${escapeHtml(source.error)}]</span>` : ''}
                </div>
            `;
        }).join('');

        return `
            <div class="card ${statusClass}">
                <h3>
                    <span class="status-indicator ${statusClass}"></span>
                    ${escapeHtml(sw.name)}
                </h3>
                <div class="card-grid">
                    <div class="card-row">
                        <span class="card-label">Status:</span>
                        <span class="${statusClass}">${statusText}</span>
                    </div>
                    <div class="card-row">
                        <span class="card-label">Logic:</span>
                        <span>${logicText}</span>
                    </div>
                    ${pendingInfo}
                    <div class="card-row">
                        <span class="card-label">Last Update:</span>
                        <span>${lastUpdate}</span>
                    </div>
                    ${sw.hold_time_seconds > 0 ? `
                    <div class="card-row">
                        <span class="card-label">Hold Time:</span>
                        <span>${sw.hold_time_seconds}s</span>
                    </div>
                    ` : ''}
                    ${sw.error ? `
                    <div class="card-row">
                        <span class="card-label">Error:</span>
                        <span style="color: #f44336;">${escapeHtml(sw.error)}</span>
                    </div>
                    ` : ''}
                    <div class="card-row" style="grid-column: 1 / -1;">
                        <span class="card-label">Sources (${sw.source_states.length}):</span>
                        <div style="margin-top: 4px;">
                            ${sourcesHtml}
                        </div>
                    </div>
                </div>
            </div>
        `;
    }).join('');
}

// Render group status
function renderGroupStatus(groups) {
    const container = document.getElementById('groups-status-list');
    const section = document.getElementById('groups-status-section');

    if (!groups || groups.length === 0) {
        section.style.display = 'none';
        return;
    }

    section.style.display = 'block';

    container.innerHTML = groups.map(group => {
        const statusClass = group.enabled ? (group.is_safe ? 'safe' : 'unsafe') : 'disabled';
        const statusText = group.enabled ? (group.is_safe ? 'SAFE' : 'UNSAFE') : 'DISABLED';

        // Format logic description
        let logicText = '';
        if (group.logic === 'any') {
            logicText = 'Unsafe if ANY member unsafe (OR)';
        } else if (group.logic === 'all') {
            logicText = 'Unsafe only if ALL members unsafe (AND)';
        } else if (group.logic && group.logic.min_of) {
            logicText = `Unsafe if at least ${group.logic.min_of.count} of ${group.member_statuses.length} members unsafe`;
        }

        // Format member states
        const membersHtml = group.member_statuses.map(member => {
            const memberClass = member.is_safe ? 'safe' : 'unsafe';
            const memberIcon = member.is_safe ? '✓' : '⚠';
            return `
                <div style="display: flex; align-items: center; gap: 8px; padding: 4px 0;">
                    <span class="status-indicator ${memberClass}" style="width: 10px; height: 10px;"></span>
                    <span>${memberIcon} ${escapeHtml(member.name)}</span>
                    ${member.error ? `<span style="color: #f44336; font-size: 0.85em;">[${escapeHtml(member.error)}]</span>` : ''}
                </div>
            `;
        }).join('');

        return `
            <div class="card ${statusClass}">
                <h3>
                    <span class="status-indicator ${statusClass}"></span>
                    ${escapeHtml(group.name)}
                </h3>
                <div class="card-grid">
                    <div class="card-row">
                        <span class="card-label">Status:</span>
                        <span class="${statusClass}">${statusText}</span>
                    </div>
                    <div class="card-row">
                        <span class="card-label">Logic:</span>
                        <span>${logicText}</span>
                    </div>
                    ${group.description ? `
                    <div class="card-row">
                        <span class="card-label">Description:</span>
                        <span>${escapeHtml(group.description)}</span>
                    </div>
                    ` : ''}
                    ${group.error ? `
                    <div class="card-row">
                        <span class="card-label">Error:</span>
                        <span style="color: #f44336;">${escapeHtml(group.error)}</span>
                    </div>
                    ` : ''}
                    <div class="card-row" style="grid-column: 1 / -1;">
                        <span class="card-label">Members (${group.member_statuses.length}):</span>
                        <div style="margin-top: 4px;">
                            ${membersHtml}
                        </div>
                    </div>
                </div>
            </div>
        `;
    }).join('');
}

// ============================================================================
// Switch Configuration
// ============================================================================

function renderSwitches() {
    const container = document.getElementById('switches-list');
    const switches = Object.values(config.switches || {});

    if (switches.length === 0) {
        container.innerHTML = '<p style="text-align: center; color: #999;">No switches configured. Create switches to aggregate multiple monitors.</p>';
        return;
    }

    container.innerHTML = switches.map(sw => {
        let logicText = '';
        if (sw.logic === 'any') {
            logicText = 'ANY (OR)';
        } else if (sw.logic === 'all') {
            logicText = 'ALL (AND)';
        } else if (sw.logic && sw.logic.min_of) {
            logicText = `MIN ${sw.logic.min_of.count}`;
        }

        return `
            <div class="list-item">
                <div class="list-item-info">
                    <strong>${escapeHtml(sw.name)}</strong> (${escapeHtml(sw.id)})<br>
                    Logic: ${logicText}, Sources: ${sw.sources.length}, Hold: ${sw.hold_time_seconds}s
                    ${!sw.enabled ? '<span style="color: #999;"> [Disabled]</span>' : ''}
                </div>
                <div class="list-item-actions">
                    <button class="btn btn-primary" onclick="editSwitch('${escapeHtml(sw.id)}')">Edit</button>
                    <button class="btn btn-danger" onclick="deleteSwitch('${escapeHtml(sw.id)}')">Delete</button>
                </div>
            </div>
        `;
    }).join('');
}

function toggleSwitchLogicOptions() {
    const logic = document.getElementById('switch-logic').value;
    document.getElementById('switch-min-count-group').style.display = logic === 'min_of' ? 'block' : 'none';
}

function populateSwitchSources() {
    const container = document.getElementById('switch-sources-container');
    const mqttMonitors = Object.values(config.mqtt_monitors || {});
    const alpacaMonitors = Object.values(config.alpaca_monitors || {});

    if (mqttMonitors.length === 0 && alpacaMonitors.length === 0) {
        container.innerHTML = '<p style="color: #999;">No monitors available. Create MQTT or Alpaca monitors first.</p>';
        return;
    }

    let html = '';

    if (mqttMonitors.length > 0) {
        html += '<div style="margin-bottom: 10px;"><strong>MQTT Monitors:</strong></div>';
        html += mqttMonitors.map(m => `
            <label style="display: block; margin: 4px 0;">
                <input type="checkbox" class="switch-source-checkbox" value="${escapeHtml(m.id)}">
                ${escapeHtml(m.name)} (${escapeHtml(m.id)})
            </label>
        `).join('');
    }

    if (alpacaMonitors.length > 0) {
        html += '<div style="margin: 10px 0;"><strong>Alpaca Monitors:</strong></div>';
        html += alpacaMonitors.map(m => `
            <label style="display: block; margin: 4px 0;">
                <input type="checkbox" class="switch-source-checkbox" value="${escapeHtml(m.id)}">
                ${escapeHtml(m.name)} (${escapeHtml(m.id)})
            </label>
        `).join('');
    }

    container.innerHTML = html;
}

function showAddSwitch() {
    currentEditId = null;
    currentEditType = 'switch';
    document.getElementById('switch-form').style.display = 'block';
    document.getElementById('switch-form-title').textContent = 'Add Switch';
    document.getElementById('switch-id').value = '';
    document.getElementById('switch-id').disabled = false;
    document.getElementById('switch-name').value = '';
    document.getElementById('switch-description').value = '';
    document.getElementById('switch-logic').value = 'any';
    document.getElementById('switch-min-count').value = '2';
    document.getElementById('switch-min-count-group').style.display = 'none';
    document.getElementById('switch-timeout').value = '300';
    document.getElementById('switch-hold-time').value = '0';
    document.getElementById('switch-enabled').checked = true;

    // Add event listener for logic dropdown
    document.getElementById('switch-logic').onchange = toggleSwitchLogicOptions;

    populateSwitchSources();
}

function editSwitch(id) {
    const sw = config.switches[id];
    if (!sw) return;

    currentEditId = id;
    currentEditType = 'switch';
    document.getElementById('switch-form').style.display = 'block';
    document.getElementById('switch-form-title').textContent = 'Edit Switch';
    document.getElementById('switch-id').value = sw.id;
    document.getElementById('switch-id').disabled = true;
    document.getElementById('switch-name').value = sw.name;
    document.getElementById('switch-description').value = sw.description || '';
    document.getElementById('switch-timeout').value = sw.timeout_seconds || 300;
    document.getElementById('switch-hold-time').value = sw.hold_time_seconds || 0;
    document.getElementById('switch-enabled').checked = sw.enabled !== false;

    // Set logic
    if (sw.logic === 'any' || sw.logic === 'all') {
        document.getElementById('switch-logic').value = sw.logic;
        document.getElementById('switch-min-count-group').style.display = 'none';
    } else if (sw.logic && sw.logic.min_of) {
        document.getElementById('switch-logic').value = 'min_of';
        document.getElementById('switch-min-count').value = sw.logic.min_of.count;
        document.getElementById('switch-min-count-group').style.display = 'block';
    }

    // Add event listener for logic dropdown
    document.getElementById('switch-logic').onchange = toggleSwitchLogicOptions;

    populateSwitchSources();

    // Check the selected sources
    setTimeout(() => {
        const checkboxes = document.querySelectorAll('.switch-source-checkbox');
        checkboxes.forEach(cb => {
            cb.checked = sw.sources.includes(cb.value);
        });
    }, 0);
}

function cancelSwitch() {
    document.getElementById('switch-form').style.display = 'none';
    currentEditId = null;
    currentEditType = null;
}

function saveSwitch() {
    const id = document.getElementById('switch-id').value.trim();
    const name = document.getElementById('switch-name').value.trim();
    const description = document.getElementById('switch-description').value.trim();
    const logicType = document.getElementById('switch-logic').value;
    const minCount = parseInt(document.getElementById('switch-min-count').value);
    const timeout_seconds = parseInt(document.getElementById('switch-timeout').value);
    const hold_time_seconds = parseInt(document.getElementById('switch-hold-time').value);
    const enabled = document.getElementById('switch-enabled').checked;

    // Get selected sources
    const checkboxes = document.querySelectorAll('.switch-source-checkbox:checked');
    const sources = Array.from(checkboxes).map(cb => cb.value);

    if (!id || !name) {
        showNotification('Please fill in ID and Name', 'error');
        return;
    }

    if (sources.length === 0) {
        showNotification('Please select at least one source monitor', 'error');
        return;
    }

    // Check for duplicate ID when adding new
    if (!currentEditId && config.switches[id]) {
        showNotification('Switch ID already exists', 'error');
        return;
    }

    // Build logic object
    let logic;
    if (logicType === 'any' || logicType === 'all') {
        logic = logicType;
    } else if (logicType === 'min_of') {
        if (minCount < 1 || minCount > sources.length) {
            showNotification(`Minimum count must be between 1 and ${sources.length}`, 'error');
            return;
        }
        logic = { min_of: { count: minCount } };
    }

    config.switches = config.switches || {};
    config.switches[id] = {
        id,
        name,
        description: description || null,
        sources,
        logic,
        timeout_seconds,
        hold_time_seconds,
        enabled
    };

    saveConfig().then(success => {
        if (success) {
            cancelSwitch();
            renderSwitches();
            showNotification('Switch saved. Click Reload to apply changes.', 'success');
        }
    });
}

function deleteSwitch(id) {
    if (!confirm(`Delete switch '${id}'?`)) return;

    delete config.switches[id];
    saveConfig().then(success => {
        if (success) {
            renderSwitches();
            showNotification('Switch deleted. Click Reload to apply changes.', 'success');
        }
    });
}

// ============================================================================
// Monitor Groups
// ============================================================================

function renderMonitorGroups() {
    const container = document.getElementById('monitor-groups-list');
    const groups = Object.values(config.monitor_groups || {});

    if (groups.length === 0) {
        container.innerHTML = '<p style="text-align: center; color: #999;">No monitor groups configured. Create groups to combine multiple monitors with custom logic.</p>';
        return;
    }

    container.innerHTML = groups.map(group => {
        let logicText = '';
        if (group.logic === 'any') {
            logicText = 'ANY (unsafe if any)';
        } else if (group.logic === 'all') {
            logicText = 'ALL (unsafe if all)';
        } else if (group.logic && group.logic.min_of) {
            logicText = `MIN ${group.logic.min_of.count} of ${group.members.length}`;
        }

        return `
            <div class="list-item">
                <div class="list-item-info">
                    <strong>${escapeHtml(group.name)}</strong> (${escapeHtml(group.id)})<br>
                    Logic: ${logicText}, Members: ${group.members.length}
                    ${!group.enabled ? '<span style="color: #999;"> [Disabled]</span>' : ''}
                </div>
                <div class="list-item-actions">
                    <button class="btn btn-primary" onclick="editMonitorGroup('${escapeHtml(group.id)}')">Edit</button>
                    <button class="btn btn-danger" onclick="deleteMonitorGroup('${escapeHtml(group.id)}')">Delete</button>
                </div>
            </div>
        `;
    }).join('');
}

function toggleGroupLogicOptions() {
    const logic = document.getElementById('group-logic').value;
    document.getElementById('group-min-count-group').style.display = logic === 'min_of' ? 'block' : 'none';
}

function populateGroupMembers() {
    const container = document.getElementById('group-members-container');
    const mqttMonitors = Object.values(config.mqtt_monitors || {});
    const alpacaMonitors = Object.values(config.alpaca_monitors || {});

    if (mqttMonitors.length === 0 && alpacaMonitors.length === 0) {
        container.innerHTML = '<p style="color: #999;">No monitors available. Create MQTT or Alpaca monitors first.</p>';
        return;
    }

    let html = '';

    if (mqttMonitors.length > 0) {
        html += '<div style="margin-bottom: 10px;"><strong>MQTT Monitors:</strong></div>';
        html += mqttMonitors.map(m => `
            <label style="display: block; margin: 4px 0;">
                <input type="checkbox" class="group-member-checkbox" value="${escapeHtml(m.id)}">
                ${escapeHtml(m.name)} (${escapeHtml(m.id)})
            </label>
        `).join('');
    }

    if (alpacaMonitors.length > 0) {
        html += '<div style="margin: 10px 0;"><strong>Alpaca Monitors:</strong></div>';
        html += alpacaMonitors.map(m => `
            <label style="display: block; margin: 4px 0;">
                <input type="checkbox" class="group-member-checkbox" value="${escapeHtml(m.id)}">
                ${escapeHtml(m.name)} (${escapeHtml(m.id)})
            </label>
        `).join('');
    }

    container.innerHTML = html;
}

function showAddMonitorGroup() {
    currentEditId = null;
    currentEditType = 'monitor-group';
    document.getElementById('monitor-group-form').style.display = 'block';
    document.getElementById('monitor-group-form-title').textContent = 'Add Monitor Group';
    document.getElementById('group-id').value = '';
    document.getElementById('group-id').disabled = false;
    document.getElementById('group-name').value = '';
    document.getElementById('group-description').value = '';
    document.getElementById('group-logic').value = 'any';
    document.getElementById('group-min-count').value = '2';
    document.getElementById('group-min-count-group').style.display = 'none';
    document.getElementById('group-enabled').checked = true;

    populateGroupMembers();
}

function editMonitorGroup(id) {
    const group = config.monitor_groups[id];
    if (!group) return;

    currentEditId = id;
    currentEditType = 'monitor-group';
    document.getElementById('monitor-group-form').style.display = 'block';
    document.getElementById('monitor-group-form-title').textContent = 'Edit Monitor Group';
    document.getElementById('group-id').value = group.id;
    document.getElementById('group-id').disabled = true;
    document.getElementById('group-name').value = group.name;
    document.getElementById('group-description').value = group.description || '';
    document.getElementById('group-enabled').checked = group.enabled !== false;

    // Set logic
    if (group.logic === 'any' || group.logic === 'all') {
        document.getElementById('group-logic').value = group.logic;
        document.getElementById('group-min-count-group').style.display = 'none';
    } else if (group.logic && group.logic.min_of) {
        document.getElementById('group-logic').value = 'min_of';
        document.getElementById('group-min-count').value = group.logic.min_of.count;
        document.getElementById('group-min-count-group').style.display = 'block';
    }

    populateGroupMembers();

    // Check the selected members
    setTimeout(() => {
        const checkboxes = document.querySelectorAll('.group-member-checkbox');
        checkboxes.forEach(cb => {
            cb.checked = group.members.includes(cb.value);
        });
    }, 0);
}

function cancelMonitorGroup() {
    document.getElementById('monitor-group-form').style.display = 'none';
    currentEditId = null;
    currentEditType = null;
}

function saveMonitorGroup() {
    const id = document.getElementById('group-id').value.trim();
    const name = document.getElementById('group-name').value.trim();
    const description = document.getElementById('group-description').value.trim();
    const logicType = document.getElementById('group-logic').value;
    const minCount = parseInt(document.getElementById('group-min-count').value);
    const enabled = document.getElementById('group-enabled').checked;

    // Get selected members
    const checkboxes = document.querySelectorAll('.group-member-checkbox:checked');
    const members = Array.from(checkboxes).map(cb => cb.value);

    if (!id || !name) {
        showNotification('Please fill in ID and Name', 'error');
        return;
    }

    if (members.length === 0) {
        showNotification('Please select at least one member monitor', 'error');
        return;
    }

    // Check for duplicate ID when adding new
    if (!currentEditId && config.monitor_groups[id]) {
        showNotification('Group ID already exists', 'error');
        return;
    }

    // Build logic object
    let logic;
    if (logicType === 'any' || logicType === 'all') {
        logic = logicType;
    } else if (logicType === 'min_of') {
        if (minCount < 1 || minCount > members.length) {
            showNotification(`Minimum count must be between 1 and ${members.length}`, 'error');
            return;
        }
        logic = { min_of: { count: minCount } };
    }

    config.monitor_groups = config.monitor_groups || {};
    config.monitor_groups[id] = {
        id,
        name,
        description: description || null,
        members,
        logic,
        enabled
    };

    saveConfig().then(success => {
        if (success) {
            cancelMonitorGroup();
            renderMonitorGroups();
            showNotification('Monitor group saved.', 'success');
        }
    });
}

function deleteMonitorGroup(id) {
    if (!confirm(`Delete monitor group '${id}'?`)) return;

    delete config.monitor_groups[id];
    saveConfig().then(success => {
        if (success) {
            renderMonitorGroups();
            showNotification('Monitor group deleted.', 'success');
        }
    });
}

// ============================================================================
// Alpaca Discovery
// ============================================================================

async function scanForDevices() {
    const statusEl = document.getElementById('discovery-status');
    const container = document.getElementById('discovered-devices-list');

    statusEl.textContent = 'Scanning...';
    container.innerHTML = '<p style="text-align: center; color: #999;">Scanning network for Alpaca devices...</p>';

    try {
        const response = await fetch('/api/discovery/scan');
        const devices = await response.json();

        statusEl.textContent = `Found ${devices.length} device(s)`;

        if (devices.length === 0) {
            container.innerHTML = '<p style="text-align: center; color: #999;">No Alpaca devices found on the network. Make sure your devices are powered on and connected.</p>';
            return;
        }

        container.innerHTML = devices.map(device => {
            const devicesHtml = device.devices.map(d => `
                <div style="margin: 5px 0; padding: 5px; background: #f5f5f5; border-radius: 4px;">
                    <strong>${escapeHtml(d.device_name)}</strong> (${escapeHtml(d.device_type)} #${d.device_number})
                    <button class="btn btn-primary" style="float: right; font-size: 0.8em; padding: 2px 8px;" onclick="addDiscoveredDevice('${escapeHtml(device.host)}', ${device.port}, '${escapeHtml(d.device_type)}', ${d.device_number}, '${escapeHtml(d.device_name)}')">Add as Monitor</button>
                </div>
            `).join('');

            return `
                <div class="card" style="margin-bottom: 15px;">
                    <h3>${escapeHtml(device.host)}:${device.port}</h3>
                    <div style="margin-top: 10px;">
                        ${device.devices.length > 0 ? devicesHtml : '<p style="color: #999;">No devices reported</p>'}
                    </div>
                </div>
            `;
        }).join('');
    } catch (error) {
        statusEl.textContent = 'Scan failed';
        container.innerHTML = `<p style="text-align: center; color: #e74c3c;">Failed to scan: ${error.message}</p>`;
        console.error('Discovery scan failed:', error);
    }
}

async function probeHost() {
    const host = document.getElementById('probe-host').value.trim();
    const port = parseInt(document.getElementById('probe-port').value);
    const resultEl = document.getElementById('probe-result');

    if (!host || !port) {
        showNotification('Please enter host and port', 'error');
        return;
    }

    resultEl.innerHTML = '<p style="color: #999;">Probing...</p>';

    try {
        const response = await fetch('/api/discovery/probe', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ host, port })
        });

        if (!response.ok) {
            const error = await response.text();
            resultEl.innerHTML = `<p style="color: #e74c3c;">No Alpaca server found at ${host}:${port}</p>`;
            return;
        }

        const device = await response.json();

        const devicesHtml = device.devices.map(d => `
            <div style="margin: 5px 0; padding: 5px; background: #f5f5f5; border-radius: 4px;">
                <strong>${escapeHtml(d.device_name)}</strong> (${escapeHtml(d.device_type)} #${d.device_number})
                <button class="btn btn-primary" style="float: right; font-size: 0.8em; padding: 2px 8px;" onclick="addDiscoveredDevice('${escapeHtml(device.host)}', ${device.port}, '${escapeHtml(d.device_type)}', ${d.device_number}, '${escapeHtml(d.device_name)}')">Add as Monitor</button>
            </div>
        `).join('');

        resultEl.innerHTML = `
            <div class="card" style="background: #e8f5e9;">
                <h3>Found Alpaca Server at ${escapeHtml(device.host)}:${device.port}</h3>
                <div style="margin-top: 10px;">
                    ${device.devices.length > 0 ? devicesHtml : '<p style="color: #999;">No devices reported</p>'}
                </div>
            </div>
        `;
    } catch (error) {
        resultEl.innerHTML = `<p style="color: #e74c3c;">Probe failed: ${error.message}</p>`;
        console.error('Probe failed:', error);
    }
}

function addDiscoveredDevice(host, port, deviceType, deviceNumber, deviceName) {
    // Generate a unique ID from the device info
    const id = `${deviceType.toLowerCase()}-${host.replace(/\./g, '-')}-${deviceNumber}`;
    const name = `${deviceName} (${host})`;

    // Pre-fill the Alpaca monitor form
    currentEditId = null;
    currentEditType = 'alpaca-monitor';
    document.getElementById('alpaca-monitor-form').style.display = 'block';
    document.getElementById('alpaca-monitor-id').value = id;
    document.getElementById('alpaca-monitor-id').disabled = false;
    document.getElementById('alpaca-monitor-name').value = name;
    document.getElementById('alpaca-monitor-host').value = host;
    document.getElementById('alpaca-monitor-port').value = port;
    document.getElementById('alpaca-monitor-device-type').value = deviceType.toLowerCase();
    document.getElementById('alpaca-monitor-device-number').value = deviceNumber;

    // Set sensible defaults based on device type
    if (deviceType.toLowerCase() === 'safetymonitor') {
        document.getElementById('alpaca-monitor-property').value = 'issafe';
        document.getElementById('alpaca-monitor-threshold').value = '1';
        document.getElementById('alpaca-monitor-operator').value = 'equal';
        document.getElementById('alpaca-monitor-safe-when-true').checked = true;
    } else {
        document.getElementById('alpaca-monitor-property').value = '';
        document.getElementById('alpaca-monitor-threshold').value = '';
    }

    document.getElementById('alpaca-monitor-timeout').value = '300';
    document.getElementById('alpaca-monitor-hold-time').value = '0';
    document.getElementById('alpaca-monitor-include-in-safety').checked = true;

    // Switch to the Alpaca Monitors tab
    switchTab('alpaca-monitors');
    showNotification('Device added to form. Please review settings and click Save.', 'success');
}
