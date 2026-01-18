
// ===== Weather Device Management =====

let currentWeatherDevice = null;
let weatherDevices = [];

async function loadWeatherDevices() {
    try {
        const response = await fetch('/api/weather/devices');
        if (!response.ok) throw new Error('Failed to load weather devices');

        weatherDevices = await response.json();
        renderWeatherDevicesList();
    } catch (error) {
        console.error('Error loading weather devices:', error);
        showNotification('Error loading weather devices: ' + error.message, 'error');
    }
}

function renderWeatherDevicesList() {
    const container = document.getElementById('weather-devices-list');
    if (!container) return;

    if (weatherDevices.length === 0) {
        container.innerHTML = '<p class="no-data">No weather devices configured. Click "Add Weather Device" to get started.</p>';
        return;
    }

    container.innerHTML = weatherDevices.map(device => `
        <div class="list-item">
            <div class="list-item-header">
                <div>
                    <span class="list-item-title">${escapeHtml(device.name)}</span>
                    <span class="badge badge-${device.source_type}">${device.source_type.toUpperCase()}</span>
                    ${device.enabled ? '<span class="badge badge-success">Enabled</span>' : '<span class="badge badge-secondary">Disabled</span>'}
                    ${device.connected ? '<span class="badge badge-info">Connected</span>' : ''}
                    ${device.safety_enabled ? '<span class="badge badge-warning">Safety</span>' : ''}
                </div>
                <div class="list-item-actions">
                    <button class="btn btn-sm btn-secondary" onclick="toggleWeatherDeviceEnabled(${device.device_number}, ${!device.enabled})">
                        ${device.enabled ? 'Disable' : 'Enable'}
                    </button>
                    <button class="btn btn-sm btn-primary" onclick="editWeatherDevice(${device.device_number})">Edit</button>
                    <button class="btn btn-sm btn-danger" onclick="deleteWeatherDevice(${device.device_number})">Delete</button>
                </div>
            </div>
            <div class="list-item-details">
                <div><strong>Device Number:</strong> ${device.device_number}</div>
                <div><strong>Description:</strong> ${escapeHtml(device.description)}</div>
                <div><strong>Auto-Connect:</strong> ${device.auto_connect ? 'Yes' : 'No'}</div>
            </div>
        </div>
    `).join('');
}

function showAddWeatherDevice() {
    currentWeatherDevice = null;
    document.getElementById('weather-device-form-title').textContent = 'Add Weather Device';
    document.getElementById('weather-device-number').disabled = false;
    resetWeatherDeviceForm();
    document.getElementById('weather-device-form').style.display = 'block';
    document.getElementById('weather-devices-list').style.display = 'none';
}

async function editWeatherDevice(deviceNumber) {
    try {
        const response = await fetch(`/api/weather/devices/${deviceNumber}`);
        if (!response.ok) throw new Error('Failed to load weather device');

        currentWeatherDevice = await response.json();
        document.getElementById('weather-device-form-title').textContent = 'Edit Weather Device';
        document.getElementById('weather-device-number').disabled = true;
        populateWeatherDeviceForm(currentWeatherDevice);
        document.getElementById('weather-device-form').style.display = 'block';
        document.getElementById('weather-devices-list').style.display = 'none';
    } catch (error) {
        console.error('Error loading weather device:', error);
        showNotification('Error loading weather device: ' + error.message, 'error');
    }
}

function resetWeatherDeviceForm() {
    document.getElementById('weather-device-number').value = 0;
    document.getElementById('weather-device-name').value = '';
    document.getElementById('weather-device-description').value = '';
    document.getElementById('weather-device-source-type').value = 'tempest';
    document.getElementById('weather-device-tempest-serial').value = '';
    document.getElementById('weather-device-wu-station').value = '';
    document.getElementById('weather-device-wu-apikey').value = '';
    document.getElementById('weather-device-enabled').checked = true;
    document.getElementById('weather-device-auto-connect').checked = true;
    document.getElementById('weather-safety-enabled').checked = false;

    // Reset all threshold fields
    const thresholds = ['temperature', 'humidity', 'wind_speed', 'wind_gust', 'rain_rate', 'sky_brightness'];
    thresholds.forEach(t => {
        document.getElementById(`weather-threshold-${t}-enabled`).checked = false;
        toggleThresholdFields(t);
    });

    toggleWeatherSafetyFields();
    updateWeatherSourceFields();
}

function populateWeatherDeviceForm(device) {
    document.getElementById('weather-device-number').value = device.device_number;
    document.getElementById('weather-device-name').value = device.name;
    document.getElementById('weather-device-description').value = device.description;
    document.getElementById('weather-device-enabled').checked = device.enabled;
    document.getElementById('weather-device-auto-connect').checked = device.auto_connect;

    // Set source type
    if (device.source.type === 'tempest') {
        document.getElementById('weather-device-source-type').value = 'tempest';
        document.getElementById('weather-device-tempest-serial').value = device.source.serial_number || '';
    } else if (device.source.type === 'weatherunderground') {
        document.getElementById('weather-device-source-type').value = 'weatherunderground';
        document.getElementById('weather-device-wu-station').value = device.source.station_id || '';
        document.getElementById('weather-device-wu-apikey').value = device.source.api_key || '';
    }
    updateWeatherSourceFields();

    // Set safety thresholds
    if (device.safety_thresholds) {
        document.getElementById('weather-safety-enabled').checked = device.safety_thresholds.enabled;
        document.getElementById('weather-safety-timeout').value = device.safety_thresholds.timeout_seconds;
        document.getElementById('weather-safety-hold-time').value = device.safety_thresholds.hold_time_seconds;

        // Populate individual thresholds
        const thresholds = ['temperature', 'humidity', 'wind_speed', 'wind_gust', 'rain_rate', 'sky_brightness'];
        thresholds.forEach(t => {
            const config = device.safety_thresholds[t];
            if (config) {
                document.getElementById(`weather-threshold-${t}-enabled`).checked = true;
                document.getElementById(`weather-threshold-${t}`).value = config.threshold;
                document.getElementById(`weather-threshold-${t}-operator`).value = config.operator;
                document.getElementById(`weather-threshold-${t}-safe-when-true`).checked = config.safe_when_true;
                toggleThresholdFields(t);
            }
        });
    } else {
        document.getElementById('weather-safety-enabled').checked = false;
    }
    toggleWeatherSafetyFields();
}

function updateWeatherSourceFields() {
    const sourceType = document.getElementById('weather-device-source-type').value;
    document.getElementById('tempest-fields').style.display = sourceType === 'tempest' ? 'block' : 'none';
    document.getElementById('wu-fields').style.display = sourceType === 'weatherunderground' ? 'block' : 'none';
}

function toggleWeatherSafetyFields() {
    const enabled = document.getElementById('weather-safety-enabled').checked;
    document.getElementById('weather-safety-fields').style.display = enabled ? 'block' : 'none';
}

function toggleThresholdFields(type) {
    const enabled = document.getElementById(`weather-threshold-${type}-enabled`).checked;
    document.getElementById(`weather-threshold-${type}-fields`).style.display = enabled ? 'block' : 'none';
}

async function saveWeatherDevice() {
    const deviceNumber = parseInt(document.getElementById('weather-device-number').value);
    const name = document.getElementById('weather-device-name').value.trim();
    const description = document.getElementById('weather-device-description').value.trim();

    if (!name) {
        showNotification('Device name is required', 'error');
        return;
    }

    const sourceType = document.getElementById('weather-device-source-type').value;
    let source;
    if (sourceType === 'tempest') {
        const serial = document.getElementById('weather-device-tempest-serial').value.trim();
        source = {
            type: 'tempest',
            serial_number: serial || null
        };
    } else {
        source = {
            type: 'weatherunderground',
            station_id: document.getElementById('weather-device-wu-station').value.trim(),
            api_key: document.getElementById('weather-device-wu-apikey').value.trim()
        };
    }

    // Build safety thresholds if enabled
    let safety_thresholds = null;
    if (document.getElementById('weather-safety-enabled').checked) {
        safety_thresholds = {
            enabled: true,
            timeout_seconds: parseInt(document.getElementById('weather-safety-timeout').value),
            hold_time_seconds: parseInt(document.getElementById('weather-safety-hold-time').value)
        };

        const thresholds = ['temperature', 'humidity', 'wind_speed', 'wind_gust', 'rain_rate', 'sky_brightness'];
        thresholds.forEach(t => {
            if (document.getElementById(`weather-threshold-${t}-enabled`).checked) {
                safety_thresholds[t] = {
                    threshold: parseFloat(document.getElementById(`weather-threshold-${t}`).value),
                    operator: document.getElementById(`weather-threshold-${t}-operator`).value,
                    safe_when_true: document.getElementById(`weather-threshold-${t}-safe-when-true`).checked
                };
            }
        });
    }

    const device = {
        device_number: deviceNumber,
        name,
        description,
        source,
        enabled: document.getElementById('weather-device-enabled').checked,
        auto_connect: document.getElementById('weather-device-auto-connect').checked,
        safety_thresholds
    };

    try {
        const url = currentWeatherDevice
            ? `/api/weather/devices/${deviceNumber}`
            : '/api/weather/devices';
        const method = currentWeatherDevice ? 'PUT' : 'POST';

        const response = await fetch(url, {
            method,
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(device)
        });

        if (!response.ok) {
            const errorText = await response.text();
            throw new Error(errorText || 'Failed to save weather device');
        }

        showNotification('Weather device saved successfully', 'success');
        cancelWeatherDevice();
        await loadWeatherDevices();
    } catch (error) {
        console.error('Error saving weather device:', error);
        showNotification('Error saving weather device: ' + error.message, 'error');
    }
}

function cancelWeatherDevice() {
    document.getElementById('weather-device-form').style.display = 'none';
    document.getElementById('weather-devices-list').style.display = 'block';
    currentWeatherDevice = null;
}

async function deleteWeatherDevice(deviceNumber) {
    if (!confirm('Are you sure you want to delete this weather device?')) return;

    try {
        const response = await fetch(`/api/weather/devices/${deviceNumber}`, {
            method: 'DELETE'
        });

        if (!response.ok) throw new Error('Failed to delete weather device');

        showNotification('Weather device deleted successfully', 'success');
        await loadWeatherDevices();
    } catch (error) {
        console.error('Error deleting weather device:', error);
        showNotification('Error deleting weather device: ' + error.message, 'error');
    }
}

async function toggleWeatherDeviceEnabled(deviceNumber, enabled) {
    try {
        const response = await fetch(`/api/weather/devices/${deviceNumber}/toggle`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ enabled })
        });

        if (!response.ok) throw new Error('Failed to toggle weather device');

        showNotification(`Weather device ${enabled ? 'enabled' : 'disabled'} successfully`, 'success');
        await loadWeatherDevices();
    } catch (error) {
        console.error('Error toggling weather device:', error);
        showNotification('Error toggling weather device: ' + error.message, 'error');
    }
}

// Load weather devices when weather tab is switched to
const originalSwitchTab = window.switchTab;
if (originalSwitchTab) {
    window.switchTab = function(tabName) {
        originalSwitchTab(tabName);
        if (tabName === 'weather-devices') {
            loadWeatherDevices();
        }
    };
}

// ===== Weather Status Display =====

async function loadWeatherStatus() {
    try {
        const response = await fetch('/api/weather/status');
        if (!response.ok) throw new Error('Failed to load weather status');
        
        const statuses = await response.json();
        renderWeatherStatus(statuses);
    } catch (error) {
        console.error('Error loading weather status:', error);
    }
}

function renderWeatherStatus(statuses) {
    const container = document.getElementById('weather-status-list');
    if (!container) return;

    if (statuses.length === 0) {
        container.innerHTML = '<p class="no-data">No weather devices with safety monitoring configured.</p>';
        return;
    }

    // Define consistent measurement order - keys must match backend
    const measurementOrder = [
        'Temperature',
        'Humidity',
        'Dew Point',
        'Pressure',
        'Wind Speed',
        'Wind Gust',
        'Wind Direction',
        'Rain Rate',
        'Sky Brightness'
    ];

    container.innerHTML = statuses.map(status => {
        const statusClass = status.is_safe ? 'safe' : 'unsafe';
        const statusText = status.is_safe ? 'SAFE' : 'UNSAFE';
        const lastUpdate = status.last_update
            ? new Date(status.last_update).toLocaleString()
            : 'Never';

        let measurementsHtml = '';
        // Sort measurements by the defined order
        const sortedMeasurements = measurementOrder
            .map(key => status.measurements[key])
            .filter(m => m !== undefined);

        if (sortedMeasurements.length > 0) {
            measurementsHtml = '<div class="measurements-grid">' +
                sortedMeasurements.map(m => {
                    const mClass = m.is_monitored ? (m.is_safe ? 'safe' : 'unsafe') : '';
                    const thresholdHtml = m.is_monitored
                        ? '<span class="measurement-threshold">Threshold: ' + m.threshold.toFixed(1) + '</span>'
                        : '';
                    return '<div class="measurement ' + mClass + '">' +
                        '<span class="measurement-name">' + m.name + '</span>' +
                        '<span class="measurement-value">' + m.value.toFixed(1) + '</span>' +
                        thresholdHtml +
                        '</div>';
                }).join('') +
                '</div>';
        }
        
        return '<div class="monitor-card ' + statusClass + '">' +
            '<div class="monitor-header">' +
                '<div>' +
                    '<span class="monitor-name">' + escapeHtml(status.name) + '</span>' +
                    '<span class="badge badge-' + statusClass + '">' + statusText + '</span>' +
                    (!status.enabled ? '<span class="badge badge-secondary">Disabled</span>' : '') +
                '</div>' +
                '<div class="monitor-info">' +
                    '<span>Device ' + status.device_number + '</span>' +
                    '<span>Updated: ' + lastUpdate + '</span>' +
                '</div>' +
            '</div>' +
            (status.error ? '<div class="monitor-error">' + escapeHtml(status.error) + '</div>' : '') +
            measurementsHtml +
            '</div>';
    }).join('');
}

// Update status refresh to include weather status
const originalRefreshStatus = window.refreshStatus;
if (originalRefreshStatus) {
    window.refreshStatus = async function() {
        await originalRefreshStatus();
        await loadWeatherStatus();
    };
}
