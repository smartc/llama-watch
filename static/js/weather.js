// ===== Weather Device Management =====

let currentWeatherDevice = null;
let weatherDevices = [];
let weatherAggregate = null;
let weatherFormMode = 'source'; // 'source' or 'aggregate'
let weatherAggregateOrder = []; // [{device_number, included}] while editing the aggregate

const WEATHER_THRESHOLDS = ['temperature', 'humidity', 'wind_speed', 'wind_gust', 'rain_rate', 'sky_brightness'];

// Keys match WeatherProperty in src/weather/observation.rs
const WEATHER_PROPERTIES = [
    ['temperature', 'Temperature'],
    ['humidity', 'Humidity'],
    ['dew_point', 'Dew Point'],
    ['pressure', 'Pressure'],
    ['wind_speed', 'Wind Speed'],
    ['wind_gust', 'Wind Gust'],
    ['wind_direction', 'Wind Direction'],
    ['rain_rate', 'Rain Rate'],
    ['sky_brightness', 'Sky Brightness'],
    ['solar_radiation', 'Solar Radiation'],
    ['uv_index', 'UV Index']
];

async function loadWeatherDevices() {
    try {
        const [devicesResponse, aggregateResponse] = await Promise.all([
            fetch('/api/weather/devices'),
            fetch('/api/weather/aggregate')
        ]);
        if (!devicesResponse.ok) throw new Error('Failed to load weather sources');
        if (!aggregateResponse.ok) throw new Error('Failed to load aggregate weather device');

        weatherDevices = await devicesResponse.json();
        weatherAggregate = await aggregateResponse.json();
        renderWeatherAggregateSummary();
        renderWeatherDevicesList();
    } catch (error) {
        console.error('Error loading weather devices:', error);
        showNotification('Error loading weather devices: ' + error.message, 'error');
    }
}

function weatherSourceName(deviceNumber) {
    const device = weatherDevices.find(d => d.device_number === deviceNumber);
    return device ? `${device.name} (#${deviceNumber})` : `#${deviceNumber}`;
}

function describeSourceOrder(order) {
    return order.length ? order.map(n => escapeHtml(weatherSourceName(n))).join(' → ') : 'None';
}

function renderWeatherAggregateSummary() {
    const container = document.getElementById('weather-aggregate-summary');
    if (!container || !weatherAggregate) return;

    const agg = weatherAggregate;
    const defaultOrder = agg.source_priority.length
        ? describeSourceOrder(agg.source_priority)
        : 'All enabled sources by device number';
    const overrides = WEATHER_PROPERTIES
        .filter(([key]) => agg.property_sources[key] && agg.property_sources[key].length)
        .map(([key, label]) => `<div>${label}: ${describeSourceOrder(agg.property_sources[key])}</div>`)
        .join('');
    const safetyEnabled = agg.safety_thresholds && agg.safety_thresholds.enabled;

    container.innerHTML = `
        <div class="list-item">
            <div class="list-item-header">
                <div>
                    <span class="list-item-title">${escapeHtml(agg.name)}</span>
                    <span class="badge badge-info">Device 0</span>
                    ${agg.exposed ? '<span class="badge badge-success">Active</span>' : '<span class="badge badge-secondary">Inactive</span>'}
                    ${agg.connected ? '<span class="badge badge-info">Connected</span>' : ''}
                    ${safetyEnabled ? '<span class="badge badge-warning">Safety</span>' : ''}
                </div>
                <div class="list-item-actions">
                    <button class="btn btn-sm btn-primary" onclick="editWeatherAggregate()">Edit</button>
                </div>
            </div>
            <div class="list-item-details">
                ${agg.exposed ? '' : '<div>Becomes active when at least one source is enabled.</div>'}
                <div><strong>Description:</strong> ${escapeHtml(agg.description)}</div>
                <div><strong>Source priority:</strong> ${defaultOrder}</div>
                ${overrides ? `<div><strong>Per-measurement overrides:</strong>${overrides}</div>` : ''}
                <div><strong>Stale after:</strong> ${agg.stale_after_seconds}s</div>
                <div><strong>Auto-Connect:</strong> ${agg.auto_connect ? 'Yes' : 'No'}</div>
            </div>
        </div>
    `;
}

function renderWeatherDevicesList() {
    const container = document.getElementById('weather-devices-list');
    if (!container) return;

    if (weatherDevices.length === 0) {
        container.innerHTML = '<p class="no-data">No weather sources configured. Click "Add Weather Source" to get started.</p>';
        return;
    }

    container.innerHTML = weatherDevices.map(device => {
        let exposure = '';
        if (device.exposed) {
            exposure = `<span class="badge badge-info">Device ${device.device_number}</span>`;
        } else if (device.enabled) {
            exposure = '<span class="badge badge-secondary">Aggregate only</span>';
        }
        return `
        <div class="list-item">
            <div class="list-item-header">
                <div>
                    <span class="list-item-title">${escapeHtml(device.name)}</span>
                    <span class="badge badge-${device.source_type}">${device.source_type.toUpperCase()}</span>
                    ${device.enabled ? '<span class="badge badge-success">Enabled</span>' : '<span class="badge badge-secondary">Disabled</span>'}
                    ${exposure}
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
    `;
    }).join('');
}

function setWeatherFormMode(mode) {
    weatherFormMode = mode;
    const isSource = mode === 'source';
    document.getElementById('weather-device-number-group').style.display = isSource ? '' : 'none';
    document.getElementById('weather-source-config').style.display = isSource ? '' : 'none';
    document.getElementById('weather-aggregate-fields').style.display = isSource ? 'none' : 'block';
}

function showWeatherForm(title) {
    document.getElementById('weather-device-form-title').textContent = title;
    document.getElementById('weather-device-form').style.display = 'block';
    document.getElementById('weather-overview').style.display = 'none';
}

function nextWeatherDeviceNumber() {
    let n = 1;
    while (weatherDevices.some(d => d.device_number === n)) n++;
    return n;
}

function showAddWeatherDevice() {
    currentWeatherDevice = null;
    setWeatherFormMode('source');
    document.getElementById('weather-device-number').disabled = false;
    resetWeatherDeviceForm();
    document.getElementById('weather-device-number').value = nextWeatherDeviceNumber();
    showWeatherForm('Add Weather Source');
}

async function editWeatherDevice(deviceNumber) {
    try {
        const response = await fetch(`/api/weather/devices/${deviceNumber}`);
        if (!response.ok) throw new Error('Failed to load weather device');

        currentWeatherDevice = await response.json();
        setWeatherFormMode('source');
        document.getElementById('weather-device-number').disabled = true;
        resetWeatherDeviceForm();
        populateWeatherDeviceForm(currentWeatherDevice);
        showWeatherForm('Edit Weather Source');
    } catch (error) {
        console.error('Error loading weather device:', error);
        showNotification('Error loading weather device: ' + error.message, 'error');
    }
}

async function editWeatherAggregate() {
    try {
        // Refresh so the source list is current
        await loadWeatherDevices();
        currentWeatherDevice = null;
        setWeatherFormMode('aggregate');
        resetWeatherDeviceForm();

        const agg = weatherAggregate;
        document.getElementById('weather-device-name').value = agg.name;
        document.getElementById('weather-device-description').value = agg.description;
        document.getElementById('weather-device-auto-connect').checked = agg.auto_connect;
        document.getElementById('weather-aggregate-stale-after').value = agg.stale_after_seconds;

        // Listed sources first in priority order, then the rest (excluded unless the list is empty = all)
        const listed = agg.source_priority.filter(n => weatherDevices.some(d => d.device_number === n));
        const unlisted = weatherDevices.map(d => d.device_number).filter(n => !listed.includes(n));
        weatherAggregateOrder = [
            ...listed.map(n => ({ device_number: n, included: true })),
            ...unlisted.map(n => ({ device_number: n, included: listed.length === 0 }))
        ];
        renderWeatherAggregateOrder();
        renderWeatherAggregateProperties(agg.property_sources);

        populateSafetyThresholds(agg.safety_thresholds);
        showWeatherForm('Edit Aggregate Weather Device');
    } catch (error) {
        console.error('Error loading aggregate weather device:', error);
        showNotification('Error loading aggregate weather device: ' + error.message, 'error');
    }
}

function renderWeatherAggregateOrder() {
    const container = document.getElementById('weather-aggregate-order');
    if (weatherAggregateOrder.length === 0) {
        container.innerHTML = '<p class="no-data">No weather sources configured.</p>';
        return;
    }
    container.innerHTML = weatherAggregateOrder.map((entry, i) => `
        <div class="form-row" style="align-items: center; margin-bottom: 6px;">
            <label class="checkbox-group" style="flex: 1;">
                <input type="checkbox" ${entry.included ? 'checked' : ''} onchange="weatherAggregateOrder[${i}].included = this.checked">
                <span>${i + 1}. ${escapeHtml(weatherSourceName(entry.device_number))}</span>
            </label>
            <button type="button" class="btn btn-sm btn-secondary" onclick="moveWeatherAggregateSource(${i}, -1)" ${i === 0 ? 'disabled' : ''}>↑</button>
            <button type="button" class="btn btn-sm btn-secondary" onclick="moveWeatherAggregateSource(${i}, 1)" ${i === weatherAggregateOrder.length - 1 ? 'disabled' : ''}>↓</button>
        </div>
    `).join('');
}

function moveWeatherAggregateSource(index, delta) {
    const target = index + delta;
    if (target < 0 || target >= weatherAggregateOrder.length) return;
    [weatherAggregateOrder[index], weatherAggregateOrder[target]] = [weatherAggregateOrder[target], weatherAggregateOrder[index]];
    renderWeatherAggregateOrder();
}

function renderWeatherAggregateProperties(propertySources) {
    const container = document.getElementById('weather-aggregate-properties');
    const sourceOptions = weatherDevices.map(d => `
        <option value="prefer:${d.device_number}">Prefer ${escapeHtml(weatherSourceName(d.device_number))}</option>
        <option value="only:${d.device_number}">Only ${escapeHtml(weatherSourceName(d.device_number))}</option>
    `).join('');

    container.innerHTML = WEATHER_PROPERTIES.map(([key, label]) => `
        <div class="form-row" style="align-items: center; margin-bottom: 6px;">
            <div style="flex: 1;">${label}</div>
            <select id="weather-aggregate-property-${key}" style="flex: 2;">
                <option value="default">Default priority</option>
                ${sourceOptions}
            </select>
        </div>
    `).join('');

    WEATHER_PROPERTIES.forEach(([key]) => {
        const sources = propertySources[key];
        if (!sources || sources.length === 0) return;
        const value = sources.length === 1 ? `only:${sources[0]}` : `prefer:${sources[0]}`;
        const select = document.getElementById(`weather-aggregate-property-${key}`);
        if ([...select.options].some(o => o.value === value)) select.value = value;
    });
}

// Default priority as saved: empty means "all sources by device number", which
// also picks up sources added later
function collectAggregateSourcePriority() {
    const included = weatherAggregateOrder.filter(e => e.included).map(e => e.device_number);
    const allByNumber = weatherDevices.map(d => d.device_number).sort((a, b) => a - b);
    const isDefault = included.length === allByNumber.length && included.every((n, i) => n === allByNumber[i]);
    return isDefault ? [] : included;
}

function collectAggregatePropertySources(sourcePriority) {
    const effectiveDefault = sourcePriority.length
        ? sourcePriority
        : weatherDevices.map(d => d.device_number).sort((a, b) => a - b);
    const propertySources = {};
    WEATHER_PROPERTIES.forEach(([key]) => {
        const value = document.getElementById(`weather-aggregate-property-${key}`).value;
        if (value === 'default') return;
        const [mode, n] = value.split(':');
        const preferred = parseInt(n);
        propertySources[key] = mode === 'only'
            ? [preferred]
            : [preferred, ...effectiveDefault.filter(d => d !== preferred)];
    });
    return propertySources;
}

function resetWeatherDeviceForm() {
    document.getElementById('weather-device-number').value = 1;
    document.getElementById('weather-device-name').value = '';
    document.getElementById('weather-device-description').value = '';
    document.getElementById('weather-device-source-type').value = 'tempest';
    document.getElementById('weather-device-tempest-serial').value = '';
    document.getElementById('weather-device-ecowitt-host').value = '';
    document.getElementById('weather-device-ecowitt-interval').value = 30;
    document.getElementById('weather-device-wu-station').value = '';
    document.getElementById('weather-device-wu-apikey').value = '';
    document.getElementById('weather-device-enabled').checked = true;
    document.getElementById('weather-device-expose').checked = true;
    document.getElementById('weather-device-auto-connect').checked = true;
    populateSafetyThresholds(null);
    updateWeatherSourceFields();
}

function populateWeatherDeviceForm(device) {
    document.getElementById('weather-device-number').value = device.device_number;
    document.getElementById('weather-device-name').value = device.name;
    document.getElementById('weather-device-description').value = device.description;
    document.getElementById('weather-device-enabled').checked = device.enabled;
    document.getElementById('weather-device-expose').checked = device.expose_device !== false;
    document.getElementById('weather-device-auto-connect').checked = device.auto_connect;

    // Set source type
    if (device.source.type === 'tempest') {
        document.getElementById('weather-device-source-type').value = 'tempest';
        document.getElementById('weather-device-tempest-serial').value = device.source.serial_number || '';
    } else if (device.source.type === 'ecowitt') {
        document.getElementById('weather-device-source-type').value = 'ecowitt';
        document.getElementById('weather-device-ecowitt-host').value = device.source.host || '';
        document.getElementById('weather-device-ecowitt-interval').value = device.source.poll_interval_seconds || 30;
    } else if (device.source.type === 'weatherunderground') {
        document.getElementById('weather-device-source-type').value = 'weatherunderground';
        document.getElementById('weather-device-wu-station').value = device.source.station_id || '';
        document.getElementById('weather-device-wu-apikey').value = device.source.api_key || '';
    }
    updateWeatherSourceFields();

    populateSafetyThresholds(device.safety_thresholds);
}

function populateSafetyThresholds(safetyThresholds) {
    document.getElementById('weather-safety-enabled').checked = !!(safetyThresholds && safetyThresholds.enabled);
    document.getElementById('weather-safety-timeout').value = safetyThresholds ? safetyThresholds.timeout_seconds : 300;
    document.getElementById('weather-safety-hold-time').value = safetyThresholds ? safetyThresholds.hold_time_seconds : 60;

    WEATHER_THRESHOLDS.forEach(t => {
        const config = safetyThresholds ? safetyThresholds[t] : null;
        document.getElementById(`weather-threshold-${t}-enabled`).checked = !!config;
        if (config) {
            document.getElementById(`weather-threshold-${t}`).value = config.threshold;
            document.getElementById(`weather-threshold-${t}-operator`).value = config.operator;
            document.getElementById(`weather-threshold-${t}-safe-when-true`).checked = config.safe_when_true;
        }
        toggleThresholdFields(t);
    });
    toggleWeatherSafetyFields();
}

function collectSafetyThresholds() {
    if (!document.getElementById('weather-safety-enabled').checked) return null;

    const safety_thresholds = {
        enabled: true,
        timeout_seconds: parseInt(document.getElementById('weather-safety-timeout').value),
        hold_time_seconds: parseInt(document.getElementById('weather-safety-hold-time').value)
    };
    WEATHER_THRESHOLDS.forEach(t => {
        if (document.getElementById(`weather-threshold-${t}-enabled`).checked) {
            safety_thresholds[t] = {
                threshold: parseFloat(document.getElementById(`weather-threshold-${t}`).value),
                operator: document.getElementById(`weather-threshold-${t}-operator`).value,
                safe_when_true: document.getElementById(`weather-threshold-${t}-safe-when-true`).checked
            };
        }
    });
    return safety_thresholds;
}

function updateWeatherSourceFields() {
    const sourceType = document.getElementById('weather-device-source-type').value;
    document.getElementById('tempest-fields').style.display = sourceType === 'tempest' ? 'block' : 'none';
    document.getElementById('ecowitt-fields').style.display = sourceType === 'ecowitt' ? 'block' : 'none';
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
    const name = document.getElementById('weather-device-name').value.trim();
    const description = document.getElementById('weather-device-description').value.trim();

    if (!name) {
        showNotification('Device name is required', 'error');
        return;
    }

    let url, method, body;
    if (weatherFormMode === 'aggregate') {
        const source_priority = collectAggregateSourcePriority();
        url = '/api/weather/aggregate';
        method = 'PUT';
        body = {
            name,
            description,
            auto_connect: document.getElementById('weather-device-auto-connect').checked,
            stale_after_seconds: parseInt(document.getElementById('weather-aggregate-stale-after').value) || 120,
            source_priority,
            property_sources: collectAggregatePropertySources(source_priority),
            safety_thresholds: collectSafetyThresholds()
        };
    } else {
        const deviceNumber = parseInt(document.getElementById('weather-device-number').value);
        if (!(deviceNumber >= 1)) {
            showNotification('Device number must be 1 or higher (0 is the aggregate device)', 'error');
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
        } else if (sourceType === 'ecowitt') {
            const host = document.getElementById('weather-device-ecowitt-host').value.trim();
            if (!host) {
                showNotification('Ecowitt gateway host is required', 'error');
                return;
            }
            source = {
                type: 'ecowitt',
                host,
                poll_interval_seconds: parseInt(document.getElementById('weather-device-ecowitt-interval').value) || 30
            };
        } else {
            source = {
                type: 'weatherunderground',
                station_id: document.getElementById('weather-device-wu-station').value.trim(),
                api_key: document.getElementById('weather-device-wu-apikey').value.trim()
            };
        }

        url = currentWeatherDevice ? `/api/weather/devices/${deviceNumber}` : '/api/weather/devices';
        method = currentWeatherDevice ? 'PUT' : 'POST';
        body = {
            device_number: deviceNumber,
            name,
            description,
            source,
            enabled: document.getElementById('weather-device-enabled').checked,
            expose_device: document.getElementById('weather-device-expose').checked,
            auto_connect: document.getElementById('weather-device-auto-connect').checked,
            safety_thresholds: collectSafetyThresholds()
        };
    }

    try {
        const response = await fetch(url, {
            method,
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(body)
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
    document.getElementById('weather-overview').style.display = 'block';
    currentWeatherDevice = null;
}

async function deleteWeatherDevice(deviceNumber) {
    if (!confirm('Are you sure you want to delete this weather source?')) return;

    try {
        const response = await fetch(`/api/weather/devices/${deviceNumber}`, {
            method: 'DELETE'
        });

        if (!response.ok) {
            const errorText = await response.text();
            throw new Error(errorText || 'Failed to delete weather device');
        }

        showNotification('Weather source deleted successfully', 'success');
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

        showNotification(`Weather source ${enabled ? 'enabled' : 'disabled'} successfully`, 'success');
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
        container.innerHTML = '<p class="no-data">No weather sources enabled.</p>';
        return;
    }

    // Define consistent measurement order - keys must match backend
    const measurementOrder = WEATHER_PROPERTIES.map(([, label]) => label);

    container.innerHTML = statuses.map(status => {
        const statusClass = status.is_safe ? 'safe' : 'unsafe';
        const statusText = status.is_safe ? 'SAFE' : 'UNSAFE';
        const lastUpdate = status.last_update
            ? new Date(status.last_update).toLocaleString()
            : 'Never';
        const isAggregate = status.kind === 'aggregate';
        let deviceLabel;
        if (isAggregate) {
            deviceLabel = 'Device 0 · Aggregate';
        } else if (status.exposed) {
            deviceLabel = 'Device ' + status.device_number;
        } else {
            deviceLabel = 'Source ' + status.device_number + ' · aggregate only';
        }

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
                    const sourceHtml = isAggregate && m.source_name
                        ? '<span class="measurement-threshold">via ' + escapeHtml(m.source_name) + '</span>'
                        : '';
                    return '<div class="measurement ' + mClass + '">' +
                        '<span class="measurement-name">' + m.name + '</span>' +
                        '<span class="measurement-value">' + m.value.toFixed(1) + '</span>' +
                        thresholdHtml +
                        sourceHtml +
                        '</div>';
                }).join('') +
                '</div>';
        }

        const safetyBadge = status.safety_monitored
            ? '<span class="badge badge-' + statusClass + '">' + statusText + '</span>'
            : '';

        return '<div class="monitor-card ' + (status.safety_monitored ? statusClass : '') + '">' +
            '<div class="monitor-header">' +
                '<div>' +
                    '<span class="monitor-name">' + escapeHtml(status.name) + '</span>' +
                    safetyBadge +
                '</div>' +
                '<div class="monitor-info">' +
                    '<span>' + deviceLabel + '</span>' +
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
