
const names={servers:'Servers',requests:'Application usage · Requests',jobs:'Application usage · Jobs',exceptions:'Exceptions',slow_requests:'Slow requests',slow_queries:'Slow queries',slow_jobs:'Slow jobs',slow_outgoing_requests:'Slow outgoing requests',cache:'Cache interactions',queues:'Queues'};
const main=document.querySelector('#cards'),status=document.querySelector('#status'),period=document.querySelector('#period');
const slowOrder = [['slowest', 'Slowest'], ['count', 'Count']];
const filters = {
    slow_requests: ['slow-requests', 'Order slow requests', slowOrder],
    slow_queries: ['slow-queries', 'Order slow queries', slowOrder],
    slow_jobs: ['slow-jobs', 'Order slow jobs', slowOrder],
    slow_outgoing_requests: ['slow-outgoing-requests', 'Order slow outgoing requests', slowOrder],
    exceptions: ['exceptions', 'Order exceptions', [['count', 'Count'], ['latest', 'Latest']]],
    requests: ['usage', 'Usage type', [['requests', 'Requests'], ['slow_requests', 'Slow requests'], ['jobs', 'Jobs']]],
};
const periodNames = {'1h': '1_hour', '6h': '6_hours', '24h': '24_hours', '7d': '7_days'};
let pageOptions = new URLSearchParams(location.search);
function selectLocationPeriod() {
    const value = pageOptions.get('period');
    const selected = Object.entries(periodNames).find(([short, name]) => value === short || value === name);
    period.value = selected?.[0] ?? '1h';
}
selectLocationPeriod();
function addFilter(card, name) {
    const filter = filters[name];
    if (!filter) return;
    const [parameter, label, options] = filter;
    const select = document.createElement('select');
    select.setAttribute('aria-label', label);
    for (const [value, text] of options) {
        const option = cell('option', text);
        option.value = value;
        select.append(option);
    }
    const selected = pageOptions.get(parameter);
    if (options.some(([value]) => value === selected)) select.value = selected;
    if (parameter === 'usage') card.firstElementChild.textContent = 'Application usage';
    select.addEventListener('change', () => updateOption(parameter, select.value));
    card.append(select);
}
function updateOption(parameter, value) {
    const changesPeriod = parameter === 'period';
    if (changesPeriod && value === '1h') {
        pageOptions.delete(parameter);
    } else {
        pageOptions.set(parameter, changesPeriod ? periodNames[value] : value);
    }
    const url = new URL(location.href);
    url.search = pageOptions.toString();
    if (changesPeriod) {
        history.pushState(null, '', url);
    } else {
        history.replaceState(null, '', url);
    }
    refresh();
}
function cell(tag,text){const e=document.createElement(tag);e.textContent=typeof text==='object'&&text!==null?(text.name??JSON.stringify(text)):String(text??'—');return e}
function meter(card,label,used,total){const item=document.createElement('div');item.className='reading';item.append(cell('p',label));const bar=document.createElement('progress');bar.max=100;bar.value=total>0?Math.min(100,used/total*100):0;bar.setAttribute('aria-label',label);item.append(bar);card.append(item)}
const selectedTrendReadings = new Map();
function serverTrend(card, key, label, readings, limit, unit) {
    const buckets = Object.entries(readings ?? {});
    if (!(limit > 0)) return;
    const namespace = 'http://www.w3.org/2000/svg';
    const svg = document.createElementNS(namespace, 'svg');
    svg.classList.add('server-trend');
    svg.setAttribute('viewBox', '0 0 600 40');
    svg.setAttribute('preserveAspectRatio', 'none');
    svg.setAttribute('role', 'img');
    svg.setAttribute('aria-label', label);
    svg.dataset.serverTrend = key;
    const commands = [];
    const samples = [];
    buckets.forEach(([timestamp, value], index) => {
        if (!Number.isFinite(value)) return;
        const x = 2 + index / Math.max(1, buckets.length - 1) * 596;
        const y = 37 - Math.min(1, Math.max(0, value / limit)) * 34;
        samples.push({timestamp, x, label: `${formatBucketTime(timestamp)} · ${value.toLocaleString()}${unit}`});
        const previous = Number.isFinite(buckets[index - 1]?.[1]);
        commands.push(`${previous ? 'L' : 'M'} ${x} ${y}`);
        if (!previous && !Number.isFinite(buckets[index + 1]?.[1])) {
            commands.push('h 0.01');
        }
    });
    if (samples.length === 0) return;
    const line = document.createElementNS(namespace, 'path');
    line.setAttribute('d', commands.join(' '));
    line.setAttribute('fill', 'none');
    line.setAttribute('stroke', 'currentColor');
    line.setAttribute('stroke-width', '3');
    line.setAttribute('stroke-linecap', 'round');
    line.setAttribute('vector-effect', 'non-scaling-stroke');
    svg.append(line);
    attachTrendReadings(card, svg, key, samples, 40);
}
function formatBucketTime(timestamp) {
    return new Date(timestamp.replace(' ', 'T') + 'Z')
        .toLocaleString(undefined, {timeZoneName: 'short'});
}
function attachTrendReadings(card, svg, key, samples, height) {
    if (samples.length === 0) {
        card.append(svg);
        return;
    }
    svg.setAttribute('tabindex', '0');
    svg.dataset.trendKey = key;
    const detail = document.createElement('output');
    detail.className = svg.classList.contains('server-trend') ? 'server-reading' : 'queue-reading';
    detail.hidden = true;
    let active = -1;
    const targets = samples.map(({label, x}, index) => {
        const left = index === 0 ? 0 : (samples[index - 1].x + x) / 2;
        const right = index === samples.length - 1 ? 600 : (x + samples[index + 1].x) / 2;
        const target = document.createElementNS(svg.namespaceURI, 'rect');
        target.setAttribute('x', left);
        target.setAttribute('y', '0');
        target.setAttribute('width', right - left);
        target.setAttribute('height', height);
        target.setAttribute('fill', 'transparent');
        const title = document.createElementNS(svg.namespaceURI, 'title');
        title.textContent = label;
        target.append(title);
        svg.append(target);
        target.addEventListener('pointerup', event => {
            if (event.pointerType !== 'mouse') selectReading(index);
        });
        return {target, text: title.textContent};
    });
    function selectReading(index) {
        targets[active]?.target.removeAttribute('data-selected');
        active = index;
        detail.hidden = index < 0;
        detail.textContent = targets[index]?.text ?? '';
        if (index < 0) {
            selectedTrendReadings.delete(key);
        } else {
            selectedTrendReadings.set(key, samples[index].timestamp);
            targets[index].target.setAttribute('data-selected', '');
        }
    }
    selectReading(samples.findIndex(sample => sample.timestamp === selectedTrendReadings.get(key)));
    svg.addEventListener('focus', () => {
        if (active < 0) selectReading(samples.length - 1);
    });
    svg.addEventListener('keydown', event => {
        const last = samples.length - 1;
        let index;
        switch (event.key) {
            case 'ArrowLeft': index = active < 0 ? last : Math.max(0, active - 1); break;
            case 'ArrowRight': index = Math.min(last, active + 1); break;
            case 'Home': index = 0; break;
            case 'End': index = last; break;
            case 'Escape': index = -1; break;
            default: return;
        }
        event.preventDefault();
        selectReading(index);
    });
    card.append(svg, detail);
}
function serverSize(megabytes, precision = 0) {
    const digits = 10 ** precision;
    if (megabytes >= 1024 * 1024) {
        return `${Math.round(megabytes / (1024 * 1024) * digits) / digits}TB`;
    }
    if (megabytes >= 1024) {
        return `${Math.round(megabytes / 1024 * digits) / digits}GB`;
    }
    return `${Math.round(megabytes * digits) / digits}MB`;
}
function servers(card, values, graphs, queryTitle) {
    for (const row of values) {
        let data;
        try {
            data = JSON.parse(row.value);
        } catch {
            card.append(cell('p', 'Invalid server snapshot'));
            continue;
        }
        const name = cell('h3', data.name);
        if (queryTitle) name.title = queryTitle;
        card.append(name);
        const status = row.recently_reported ? 'Recently reported' : 'Stale';
        card.append(cell('small', `${status} · Last reported ${new Date(Number(row.timestamp) * 1000).toLocaleString()}`));
        const series = Object.hasOwn(graphs, row.key) ? graphs[row.key] : undefined;
        meter(card, `CPU · ${data.cpu}%`, Number(data.cpu), 100);
        serverTrend(card, `cpu:${row.key}`, `CPU history for ${data.name}`, series?.cpu, 100, '%');
        meter(card, `Memory · ${serverSize(data.memory_used, 1)} / ${serverSize(data.memory_total, 1)}`, Number(data.memory_used), Number(data.memory_total));
        serverTrend(card, `memory:${row.key}`, `Memory history for ${data.name}`, series?.memory, Number(data.memory_total), ' MiB');
        for (const disk of data.storage ?? []) {
            meter(card, `${disk.directory} · ${serverSize(disk.used)} / ${serverSize(disk.total)}`, Number(disk.used), Number(disk.total));
        }
    }
}
const queueSeries = [
    ['queued', 'Queued', '#38bdf8'],
    ['processing', 'Processing', '#c084fc'],
    ['processed', 'Processed', '#34d399'],
    ['released', 'Released', '#fbbf24'],
    ['failed', 'Failed', '#fb7185'],
];

function queues(card, values, rate) {
    const entries = Object.entries(values);
    const connections = new Set(entries.map(([key]) => key.split(':', 1)[0]));
    const showConnection = connections.size > 1;
    const sampled = Number.isFinite(rate) && rate > 0 && rate < 1;
    const format = new Intl.NumberFormat('en-US', {maximumFractionDigits: 0});
    const count = raw => sampled ? '~' + format.format(raw / rate) : format.format(raw);
    const readingCount = raw => sampled ? '~' + (raw / rate).toLocaleString() : raw.toLocaleString();
    for (const [key, types] of entries) {
        const label = showConnection ? key : key.slice(key.indexOf(':') + 1);
        const series = queueSeries.map(([type, name, color]) => {
            const readings = Object.values(types[type] ?? {}).map(value => Number(value ?? 0));
            return {type, name, color, readings};
        });
        const maximum = Math.max(0, ...series.flatMap(({readings}) => readings));
        const peak = cell('small', 'Peak: ' + count(maximum));
        peak.className = 'queue-peak';
        if (sampled) peak.title = `Sample rate: ${rate}, Raw value: ${format.format(maximum)}`;
        card.append(cell('h3', label), peak);

        const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
        svg.classList.add('queue-trend');
        svg.setAttribute('viewBox', '0 0 600 100');
        svg.setAttribute('role', 'img');
        svg.setAttribute('aria-label', label + ' queue activity');
        const legend = document.createElement('div');
        legend.className = 'legend';
        const scale = Math.max(1, maximum);
        for (const {type, name, readings, color} of series) {
            const coordinates = readings.map((value, index) => {
                const x = index / Math.max(1, readings.length - 1) * 600;
                return `${x} ${95 - value / scale * 90}`;
            });
            const commands = [];
            for (let index = 1; index < readings.length; index++) {
                if (readings[index - 1] !== 0 || readings[index] !== 0) {
                    commands.push(`M ${coordinates[index - 1]} L ${coordinates[index]}`);
                }
            }
            if (readings.length === 1 && readings[0] !== 0) {
                commands.push(`M ${coordinates[0]} h 0.01`);
            }
            const line = document.createElementNS(svg.namespaceURI, 'path');
            line.dataset.queueSeries = type;
            line.setAttribute('d', commands.join(' '));
            line.setAttribute('fill', 'none');
            line.setAttribute('stroke', color);
            line.setAttribute('stroke-width', '2');
            line.setAttribute('stroke-linecap', 'round');
            svg.append(line);
            const total = readings.reduce((sum, value) => sum + value, 0);
            const legendLabel = cell('span', `${name}: ${count(total)}`);
            legendLabel.style.borderColor = color;
            legend.append(legendLabel);
        }
        const buckets = Object.keys(types.queued ?? {});
        const samples = buckets.map((timestamp, index) => {
            const x = index / Math.max(1, buckets.length - 1) * 600;
            const readings = queueSeries.map(([type, name]) => {
                const raw = Number(types[type]?.[timestamp] ?? 0);
                return `${name}: ${readingCount(raw)}`;
            });
            return {timestamp, x, label: `${formatBucketTime(timestamp)} · ${readings.join(', ')}`};
        });
        attachTrendReadings(card, svg, 'queue:' + key, samples, 100);
        card.append(legend);
    }
}
const countFormat = new Intl.NumberFormat('en-US', {maximumFractionDigits: 0});
function renderCount(td, raw, rate) {
    const value = typeof raw === 'string' && raw.trim() !== '' ? Number(raw) : raw;
    if (!Number.isFinite(value)) return;
    if (rate > 0 && rate < 1) {
        td.textContent = '~' + countFormat.format(value * (1 / rate));
        td.title = `Sample rate: ${rate}, Raw value: ${countFormat.format(value)}`;
    } else {
        td.textContent = countFormat.format(value);
    }
}
function renderUser(td, user) {
    const profile = document.createElement('div');
    profile.className = 'usage-user';
    if (user.avatar && user.avatar !== '0') {
        const image = document.createElement('img');
        image.src = user.avatar;
        image.alt = user.name;
        image.loading = 'lazy';
        profile.append(image);
    }
    const details = document.createElement('div');
    const name = cell('span', user.name);
    name.title = user.name;
    details.append(name);
    if (user.extra) {
        const extra = cell('small', user.extra);
        extra.title = user.name;
        details.append(extra);
    }
    profile.append(details);
    td.replaceChildren(profile);
}
function renderRows(card, values, headings = {}, countRate) {
    const box = document.createElement('div');
    box.className = 'rows';
    const table = document.createElement('table');
    const head = document.createElement('tr');
    const columns = Object.keys(values[0]);
    for (const column of columns) head.append(cell('th', headings[column] ?? column));
    table.append(head);
    for (const row of values) {
        const tr = document.createElement('tr');
        for (const column of columns) {
            const value = row[column];
            const td = cell('td', value);
            if (column === 'count' && Number.isFinite(countRate)) {
                renderCount(td, value, countRate);
            } else if (column === 'user' && typeof value?.name === 'string') {
                renderUser(td, value);
            } else if (headings[column] && value?.title) {
                td.title = value.title;
            }
            td.dataset.column = column;
            tr.append(td);
        }
        table.append(tr);
    }
    box.append(table);
    card.append(box);
}
function cacheHitRate(hits, misses) {
    const total = hits + misses;
    return total > 0 ? Math.trunc(hits / total * 10000) / 100 + '%' : '—';
}
function renderCache(card, rows, {hits, misses, sample_rate: rate}) {
    const format = new Intl.NumberFormat('en-US', {maximumFractionDigits: 0});
    const estimated = rate > 0 && rate < 1;
    const count = raw => estimated ? '~' + format.format(raw / rate) : format.format(raw);
    const rawTitle = raw => `Sample rate: ${rate}, raw count: ${format.format(raw)}`;
    const countCell = raw => ({name: count(raw), title: estimated ? rawTitle(raw) : undefined});
    const summary = document.createElement('dl');
    summary.className = 'cache-summary';
    for (const [label, value, raw] of [
        ['Hits', count(hits), hits],
        ['Misses', count(misses), misses],
        ['Hit rate', cacheHitRate(hits, misses)],
    ]) {
        const item = document.createElement('div');
        const result = cell('dd', value);
        if (estimated && raw !== undefined) result.title = rawTitle(raw);
        item.append(cell('dt', label), result);
        summary.append(item);
    }
    card.append(summary);
    if (rows.length) {
        renderRows(card, rows.slice(0, 100).map(row => {
            const rowHits = Number(row.cache_hit ?? 0);
            const rowMisses = Number(row.cache_miss ?? 0);
            return {key: row.key, hits: countCell(rowHits), misses: countCell(rowMisses),
                    hit_rate: cacheHitRate(rowHits, rowMisses)};
        }), {key: 'Key', hits: 'Hits', misses: 'Misses', hit_rate: 'Hit rate'});
    }
    if (rows.length > 100) card.append(cell('small', 'Limited to 100 entries'));
}
function queryTimingTitle(timing, label = '') {
    if (!Number.isFinite(timing?.time) || typeof timing.run_at !== 'string') return '';
    const date = new Date(timing.run_at.replace(' ', 'T') + 'Z');
    if (!Number.isFinite(date.getTime())) return '';
    const duration = timing.time.toLocaleString('en-US', {maximumFractionDigits: 0});
    const runAt = date.toLocaleString(undefined, {
        year: '2-digit', month: '2-digit', day: '2-digit',
        hourCycle: 'h24', hour: '2-digit', minute: '2-digit', second: '2-digit',
        timeZoneName: 'short',
    });
    return `${label}Time: ${duration}ms; ${label}Run at: ${runAt};`;
}

const cardNodes = new Map();
const mountedCards = new Set();
const initialLoads = new Map();
let activeRefresh;

function createCard(name) {
    const card = document.createElement('section');
    card.className = 'card';
    card.dataset.card = name;
    card.append(cell('h2', names[name]));
    return card;
}

function renderCard(name, data) {
    const previous = cardNodes.get(name);
    const active = previous.contains(document.activeElement) ? document.activeElement : null;
    const earlierTrends = previous.querySelectorAll('[data-trend-key]');
    const timings = data.query_timing ?? {};
    const card = createCard(name);
    const heading = card.firstElementChild;
    const queryTitle = name === 'cache'
        ? [queryTimingTitle(timings.cache_totals, 'Global '), queryTimingTitle(timings.cache, 'Key ')]
            .filter(Boolean).join(' ')
        : queryTimingTitle(timings[name]);
    if (queryTitle) heading.title = queryTitle;
    addFilter(card, name);
    const values = data[name];
    const cache = data.cache_totals;
    if (name === 'cache' && cache && cache.hits + cache.misses > 0) {
        renderCache(card, values ?? [], cache);
    } else if (!values || Object.keys(values).length === 0) {
        const empty = cell('div', 'No data for this period');
        empty.className = 'empty';
        card.append(empty);
    } else if (name === 'servers') {
        servers(card, values, data.server_graphs ?? {}, queryTitle);
    } else if (name === 'queues') {
        queues(card, values, data.queue_sample_rate);
    } else if (Array.isArray(values)) {
        renderRows(card, values, undefined, data.sample_rates?.[name]);
    } else {
        for (const [key, types] of Object.entries(values)) {
            card.append(cell('h3', key));
            for (const [type, readings] of Object.entries(types)) {
                const total = Object.values(readings).reduce((sum, value) => sum + Number(value ?? 0), 0);
                card.append(cell('p', `${type}: ${total}`));
            }
        }
    }
    previous.replaceChildren(...card.childNodes);
    previous.removeAttribute('aria-busy');
    const trends = new Map(Array.from(previous.querySelectorAll('[data-trend-key]'),
        trend => [trend.dataset.trendKey, trend]));
    for (const trend of earlierTrends) {
        if (!trends.has(trend.dataset.trendKey)) selectedTrendReadings.delete(trend.dataset.trendKey);
    }
    if (active?.dataset.trendKey) {
        trends.get(active.dataset.trendKey)?.focus({preventScroll: true});
    } else if (active?.matches('select[aria-label]')) {
        [...previous.querySelectorAll('select')]
            .find(select => select.getAttribute('aria-label') === active.getAttribute('aria-label'))
            ?.focus({preventScroll: true});
    }
}

async function loadCards(requested, signal) {
    const url = new URL(location.href);
    url.pathname = url.pathname.replace(/\/$/, '') + '/api/metrics';
    const options = new URLSearchParams(pageOptions);
    options.set('period', period.value);
    if (requested.length === cardNodes.size) {
        options.delete('cards');
    } else {
        options.set('cards', requested.join(','));
    }
    url.search = options.toString();
    const response = await fetch(url, {
        credentials: 'same-origin',
        headers: {Accept: 'application/json'},
        signal,
    });
    if (!response.ok) throw new Error(response.status === 403 ? 'Access denied' : 'Unable to load metrics');
    const data = await response.json();
    if (signal.aborted) return;
    for (const name of requested) renderCard(name, data);
    status.textContent = 'Updated ' + new Date().toLocaleTimeString();
}

function showCardFailure(name, message) {
    const card = cardNodes.get(name);
    card.removeAttribute('aria-busy');
    let error = card.querySelector('[data-load-status]');
    if (!error) {
        error = cell('div', '');
        error.className = 'empty';
        error.dataset.loadStatus = '';
        card.append(error);
    }
    error.textContent = message;
    status.textContent = message;
}

async function loadOne(name, signal) {
    try {
        await loadCards([name], signal);
    } catch (error) {
        if (!signal.aborted) showCardFailure(name, error.message);
    }
}

async function mountCard(name) {
    mountedCards.add(name);
    const controller = new AbortController();
    initialLoads.set(name, controller);
    await loadOne(name, controller.signal);
    if (initialLoads.get(name) === controller) initialLoads.delete(name);
}

async function refresh(automatic = false) {
    const requested = [...mountedCards].filter(name => !automatic || (
        !initialLoads.has(name) && (!document.hidden || Math.random() >= 0.95)
    ));
    if (requested.length === 0) return;
    activeRefresh?.abort();
    if (!automatic) {
        for (const pending of initialLoads.values()) pending.abort();
        initialLoads.clear();
    }
    const controller = new AbortController();
    activeRefresh = controller;
    try {
        await loadCards(requested, controller.signal);
    } catch (error) {
        if (controller.signal.aborted) return;
        if (requested.length > 1 && error.message !== 'Access denied') {
            await Promise.all(requested.map(name => loadOne(name, controller.signal)));
        } else {
            for (const name of requested) showCardFailure(name, error.message);
        }
    } finally {
        if (activeRefresh === controller) activeRefresh = undefined;
    }
}

const observer = new IntersectionObserver(entries => {
    for (const entry of entries) {
        if (!entry.isIntersecting) continue;
        observer.unobserve(entry.target);
        mountCard(entry.target.dataset.card);
    }
});
for (const name of Object.keys(names)) {
    const card = createCard(name);
    const pending = cell('div', 'Loading…');
    pending.className = 'empty';
    pending.dataset.loadStatus = '';
    card.setAttribute('aria-busy', 'true');
    card.append(pending);
    cardNodes.set(name, card);
    main.append(card);
    observer.observe(card);
}
period.addEventListener('change', () => updateOption('period', period.value));
window.addEventListener('popstate', () => {
    pageOptions = new URLSearchParams(location.search);
    selectLocationPeriod();
    refresh();
});
document.querySelector('#refresh').addEventListener('click', () => refresh());
const polling = setInterval(() => {
    if (!main.isConnected) {
        clearInterval(polling);
        return;
    }
    if (activeRefresh || !navigator.onLine) return;
    refresh(true);
}, 5000);
