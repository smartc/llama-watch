// ===== Weather Device Management =====

let currentWeatherDevice = null;
let weatherDevices = [];
let weatherAggregate = null;
let weatherFormMode = 'source'; // 'source' or 'aggregate'
// Source order rows while editing the aggregate: 'default' plus any measurement whose
// order differs from the default. Each row is [{device_number, included}].
let sourceRows = {};
let weatherSourceValues = {}; // device_number -> measurement key -> current value text
let alpacaProperties = []; // [{property, label, value, description, checked}] while editing an Alpaca source

const WEATHER_THRESHOLDS = ['temperature', 'humidity', 'wind_speed', 'wind_gust', 'rain_rate', 'sky_brightness',
    'sky_quality', 'sky_temperature', 'cloud_cover'];

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
    ['sky_quality', 'Sky Quality'],
    ['sky_temperature', 'Sky Temperature'],
    ['cloud_cover', 'Cloud Cover'],
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
                <div><strong>Stale after:</strong> ${agg.stale_after_seconds}s · <strong>Return after:</strong> ${agg.return_after_seconds}s</div>
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
        document.getElementById('weather-aggregate-return-after').value = agg.return_after_seconds;

        await loadWeatherSourceValues();
        loadSourceRows(agg);
        renderSourceRows();

        populateSafetyThresholds(agg.safety_thresholds);
        showWeatherForm('Edit Aggregate Weather Device');
    } catch (error) {
        console.error('Error loading aggregate weather device:', error);
        showNotification('Error loading aggregate weather device: ' + error.message, 'error');
    }
}

async function loadWeatherSourceValues() {
    weatherSourceValues = {};
    try {
        const response = await fetch('/api/weather/status');
        if (!response.ok) return;
        for (const status of await response.json()) {
            if (status.kind !== 'source') continue;
            const values = {};
            WEATHER_PROPERTIES.forEach(([key, label]) => {
                const m = status.measurements[label];
                if (!m) return;
                const unit = (m.name.match(/\(([^)]*)\)\s*$/) || [])[1] || '';
                values[key] = m.value.toFixed(1) + (unit ? ' ' + unit : '');
            });
            weatherSourceValues[status.device_number] = values;
        }
    } catch (error) {
        console.warn('Could not load current weather values:', error);
    }
}

// Sources that provide a measurement ('default' = all sources), by device number
function sourcesProviding(key) {
    return weatherDevices
        .filter(d => key === 'default' || (d.properties || []).includes(key))
        .map(d => d.device_number);
}

function loadSourceRows(agg) {
    sourceRows = {};
    // Default: listed sources in priority order, then the rest (excluded unless the list is empty = all)
    const all = sourcesProviding('default');
    const listed = agg.source_priority.filter(n => all.includes(n));
    sourceRows.default = [
        ...listed.map(n => ({ device_number: n, included: true })),
        ...all.filter(n => !listed.includes(n)).map(n => ({ device_number: n, included: listed.length === 0 }))
    ];
    for (const [key, list] of Object.entries(agg.property_sources || {})) {
        const providers = sourcesProviding(key);
        const listedHere = list.filter(n => providers.includes(n));
        if (listedHere.length === 0) continue;
        sourceRows[key] = [
            ...listedHere.map(n => ({ device_number: n, included: true })),
            ...providers.filter(n => !listedHere.includes(n)).map(n => ({ device_number: n, included: false }))
        ];
    }
}

// A measurement's row: its own order if changed, otherwise the default order
// restricted to the sources that provide it
function effectiveSourceRow(key) {
    if (sourceRows[key]) return sourceRows[key];
    const providers = sourcesProviding(key);
    return sourceRows.default.filter(e => providers.includes(e.device_number)).map(e => ({ ...e }));
}

function sourceRowAction(key, index, action) {
    const row = effectiveSourceRow(key).map(e => ({ ...e }));
    if (action === 'toggle') {
        if (row[index].included && row.filter(e => e.included).length === 1) {
            showNotification('At least one source must be used', 'error');
            return;
        }
        row[index].included = !row[index].included;
    } else {
        const target = index + action;
        if (target < 0 || target >= row.length) return;
        [row[index], row[target]] = [row[target], row[index]];
    }
    sourceRows[key] = row;
    renderSourceRows();
}

function resetSourceRow(key) {
    delete sourceRows[key];
    renderSourceRows();
}

function sourceRowHtml(key, label) {
    const row = effectiveSourceRow(key);
    if (row.length === 0) return '';
    const inherited = key !== 'default' && !sourceRows[key];
    let rank = 0;
    const chips = row.map((entry, i) => {
        const device = weatherDevices.find(d => d.device_number === entry.device_number);
        const name = device ? device.name : `#${entry.device_number}`;
        const value = key === 'default' ? '' : ((weatherSourceValues[entry.device_number] || {})[key] || '');
        const disabled = device && !device.enabled;
        if (entry.included) rank++;
        return `<span class="source-chip${entry.included ? '' : ' excluded'}${disabled ? ' disabled' : ''}">` +
            `<button type="button" class="chip-move" onclick="sourceRowAction('${key}', ${i}, -1)" ${i === 0 ? 'disabled' : ''} title="Move earlier">‹</button>` +
            `<button type="button" class="chip-toggle" onclick="sourceRowAction('${key}', ${i}, 'toggle')" title="${entry.included ? 'Click to stop using this source' : 'Click to use this source'}">` +
                `<span class="chip-rank">${entry.included ? rank : '–'}</span>${escapeHtml(name)}` +
                (value ? ` <small>${escapeHtml(value)}</small>` : '') +
                (disabled ? ' <small>(disabled)</small>' : '') +
            `</button>` +
            `<button type="button" class="chip-move" onclick="sourceRowAction('${key}', ${i}, 1)" ${i === row.length - 1 ? 'disabled' : ''} title="Move later">›</button>` +
            `</span>`;
    }).join('');
    let status = '';
    if (inherited) {
        status = '<span class="source-row-note">default order</span>';
    } else if (key !== 'default') {
        status = `<button type="button" class="btn btn-sm btn-secondary" onclick="resetSourceRow('${key}')">Reset</button>`;
    }
    return `<div class="source-row${inherited ? ' inherited' : ''}">` +
        `<div class="source-row-label">${label}</div>` +
        `<div class="source-row-chips">${chips}</div>` +
        `<div class="source-row-status">${status}</div>` +
        `</div>`;
}

function renderSourceRows() {
    const defaultContainer = document.getElementById('weather-aggregate-order');
    if (sourceRows.default.length === 0) {
        defaultContainer.innerHTML = '<p class="no-data">No weather sources configured.</p>';
        document.getElementById('weather-aggregate-properties').innerHTML = '';
        return;
    }
    defaultContainer.innerHTML = sourceRowHtml('default', 'All measurements');
    document.getElementById('weather-aggregate-properties').innerHTML =
        WEATHER_PROPERTIES.map(([key, label]) => sourceRowHtml(key, label)).join('');
}

// Default priority as saved: empty means "all sources by device number", which
// also picks up sources added later
function collectAggregateSourcePriority() {
    const included = sourceRows.default.filter(e => e.included).map(e => e.device_number);
    const allByNumber = weatherDevices.map(d => d.device_number).sort((a, b) => a - b);
    const isDefault = included.length === allByNumber.length && included.every((n, i) => n === allByNumber[i]);
    return isDefault ? [] : included;
}

// Only measurements whose order differs from the default are saved
function collectAggregatePropertySources() {
    const propertySources = {};
    WEATHER_PROPERTIES.forEach(([key]) => {
        if (!sourceRows[key]) return;
        const providers = sourcesProviding(key);
        const own = sourceRows[key].filter(e => e.included).map(e => e.device_number);
        const inherited = sourceRows.default
            .filter(e => e.included && providers.includes(e.device_number))
            .map(e => e.device_number);
        const same = own.length === inherited.length && own.every((n, i) => n === inherited[i]);
        if (!same) propertySources[key] = own;
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
    document.getElementById('weather-device-alpaca-host').value = '';
    document.getElementById('weather-device-alpaca-port').value = 11111;
    document.getElementById('weather-device-alpaca-device').value = 0;
    document.getElementById('weather-device-alpaca-interval').value = 30;
    alpacaProperties = [];
    renderAlpacaProperties();
    document.getElementById('weather-device-wu-station').value = '';
    document.getElementById('weather-device-wu-apikey').value = '';
    document.getElementById('weather-device-wu-interval').value = 120;
    document.getElementById('weather-device-wu-altitude').value = '';
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
    } else if (device.source.type === 'alpaca') {
        document.getElementById('weather-device-source-type').value = 'alpaca';
        document.getElementById('weather-device-alpaca-host').value = device.source.host || '';
        document.getElementById('weather-device-alpaca-port').value = device.source.port || 11111;
        document.getElementById('weather-device-alpaca-device').value = device.source.device_number || 0;
        document.getElementById('weather-device-alpaca-interval').value = device.source.poll_interval_seconds || 30;
        alpacaProperties = (device.source.properties || []).map(property => ({
            property,
            label: weatherPropertyLabel(property),
            checked: true
        }));
        renderAlpacaProperties();
    } else if (device.source.type === 'weatherunderground') {
        document.getElementById('weather-device-source-type').value = 'weatherunderground';
        document.getElementById('weather-device-wu-station').value = device.source.station_id || '';
        document.getElementById('weather-device-wu-apikey').value = device.source.api_key || '';
        document.getElementById('weather-device-wu-interval').value = device.source.poll_interval_seconds || 120;
        document.getElementById('weather-device-wu-altitude').value =
            device.source.pressure_altitude_m != null ? device.source.pressure_altitude_m : '';
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
    document.getElementById('alpaca-fields').style.display = sourceType === 'alpaca' ? 'block' : 'none';
    document.getElementById('wu-fields').style.display = sourceType === 'weatherunderground' ? 'block' : 'none';
}

function weatherPropertyLabel(key) {
    const entry = WEATHER_PROPERTIES.find(([k]) => k === key);
    return entry ? entry[1] : key;
}

function renderAlpacaProperties() {
    const container = document.getElementById('weather-device-alpaca-properties');
    if (alpacaProperties.length === 0) {
        container.innerHTML = '<p class="no-data">No properties selected yet. Click "Detect Properties".</p>';
        return;
    }
    container.innerHTML = alpacaProperties.map((p, i) => {
        const value = p.value !== undefined ? `<strong>${p.value}</strong>` : '';
        let note = '';
        if (p.description) {
            note = escapeHtml(p.description);
        } else if (p.value !== undefined) {
            note = '<em>The device doesn\'t describe this sensor; check the value is real before using it</em>';
        }
        return `
            <div class="priority-row">
                <label class="checkbox-group">
                    <input type="checkbox" ${p.checked ? 'checked' : ''} onchange="alpacaProperties[${i}].checked = this.checked">
                    <span>${escapeHtml(p.label)}</span>
                </label>
                <div style="flex: 3;">${value} <span class="info-text">${note}</span></div>
            </div>
        `;
    }).join('');
}

async function detectAlpacaProperties() {
    const host = document.getElementById('weather-device-alpaca-host').value.trim();
    if (!host) {
        showNotification('Alpaca host is required', 'error');
        return;
    }
    const container = document.getElementById('weather-device-alpaca-properties');
    container.innerHTML = '<p class="no-data">Reading device...</p>';
    try {
        const response = await fetch('/api/weather/alpaca/detect', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
                host,
                port: parseInt(document.getElementById('weather-device-alpaca-port').value) || 11111,
                device_number: parseInt(document.getElementById('weather-device-alpaca-device').value) || 0
            })
        });
        if (!response.ok) throw new Error(await response.text());
        const detected = await response.json();

        // Keep earlier choices; otherwise select properties the device describes
        const previous = new Map(alpacaProperties.map(p => [p.property, p.checked]));
        alpacaProperties = detected.properties.map(p => ({
            property: p.property,
            label: weatherPropertyLabel(p.property),
            value: p.value,
            description: p.description,
            checked: previous.has(p.property) ? previous.get(p.property) : p.description !== null
        }));
        renderAlpacaProperties();

        const nameInput = document.getElementById('weather-device-name');
        if (!nameInput.value.trim() && detected.name) nameInput.value = detected.name;
        const descriptionInput = document.getElementById('weather-device-description');
        if (!descriptionInput.value.trim() && detected.description) descriptionInput.value = detected.description;
    } catch (error) {
        console.error('Error detecting Alpaca properties:', error);
        alpacaProperties = [];
        renderAlpacaProperties();
        showNotification('Could not read Alpaca device: ' + error.message, 'error');
    }
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
            stale_after_seconds: parseInt(document.getElementById('weather-aggregate-stale-after').value) || 300,
            return_after_seconds: (v => isNaN(v) ? 300 : v)(parseInt(document.getElementById('weather-aggregate-return-after').value)),
            source_priority,
            property_sources: collectAggregatePropertySources(),
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
        } else if (sourceType === 'alpaca') {
            const host = document.getElementById('weather-device-alpaca-host').value.trim();
            const properties = alpacaProperties.filter(p => p.checked).map(p => p.property);
            if (!host) {
                showNotification('Alpaca host is required', 'error');
                return;
            }
            if (properties.length === 0) {
                showNotification('Select at least one property (use "Detect Properties")', 'error');
                return;
            }
            source = {
                type: 'alpaca',
                host,
                port: parseInt(document.getElementById('weather-device-alpaca-port').value) || 11111,
                device_number: parseInt(document.getElementById('weather-device-alpaca-device').value) || 0,
                poll_interval_seconds: parseInt(document.getElementById('weather-device-alpaca-interval').value) || 30,
                properties
            };
        } else {
            const station_id = document.getElementById('weather-device-wu-station').value.trim();
            const api_key = document.getElementById('weather-device-wu-apikey').value.trim();
            const poll_interval_seconds = parseInt(document.getElementById('weather-device-wu-interval').value) || 120;
            if (!station_id || !api_key) {
                showNotification('Weather Underground station ID and API key are required', 'error');
                return;
            }
            if (poll_interval_seconds < 60) {
                showNotification('Weather Underground poll interval must be at least 60 seconds', 'error');
                return;
            }
            const altitudeText = document.getElementById('weather-device-wu-altitude').value.trim();
            const pressure_altitude_m = altitudeText === '' ? null : parseFloat(altitudeText);
            if (pressure_altitude_m !== null && !(pressure_altitude_m >= 0 && pressure_altitude_m <= 9000)) {
                showNotification('Station altitude must be between 0 and 9000 m', 'error');
                return;
            }
            source = { type: 'weatherunderground', station_id, api_key, poll_interval_seconds, pressure_altitude_m };
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
