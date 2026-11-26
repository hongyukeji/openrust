
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
const pageOptions = new URLSearchParams(location.search);
if ([...period.options].some(option => option.value === pageOptions.get('period'))) {
    period.value = pageOptions.get('period');
}
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
    pageOptions.set(parameter, value);
    const url = new URL(location.href);
    url.search = pageOptions.toString();
    history.replaceState(null, '', url);
    refresh();
}
function cell(tag,text){const e=document.createElement(tag);e.textContent=typeof text==='object'&&text!==null?(text.name??JSON.stringify(text)):String(text??'—');return e}
function meter(card,label,used,total){const item=document.createElement('div');item.className='reading';item.append(cell('p',label));const bar=document.createElement('progress');bar.max=100;bar.value=total>0?Math.min(100,used/total*100):0;bar.setAttribute('aria-label',label);item.append(bar);card.append(item)}
const selectedServerReadings = new Map();
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
    svg.setAttribute('tabindex', '0');
    svg.dataset.serverTrend = key;
    const commands = [];
    const samples = [];
    buckets.forEach(([timestamp, value], index) => {
        if (!Number.isFinite(value)) return;
        const x = 2 + index / Math.max(1, buckets.length - 1) * 596;
        const y = 37 - Math.min(1, Math.max(0, value / limit)) * 34;
        samples.push({timestamp, value, x});
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
    const detail = document.createElement('output');
    detail.className = 'server-reading';
    detail.hidden = true;
    let active = -1;
    const targets = samples.map(({timestamp, value, x}, index) => {
        const left = index === 0 ? 0 : (samples[index - 1].x + x) / 2;
        const right = index === samples.length - 1 ? 600 : (x + samples[index + 1].x) / 2;
        const target = document.createElementNS(namespace, 'rect');
        target.setAttribute('x', left);
        target.setAttribute('y', '0');
        target.setAttribute('width', right - left);
        target.setAttribute('height', '40');
        target.setAttribute('fill', 'transparent');
        const title = document.createElementNS(namespace, 'title');
        const date = new Date(timestamp.replace(' ', 'T') + 'Z');
        title.textContent = `${date.toLocaleString(undefined, {timeZoneName: 'short'})} · ${value.toLocaleString()}${unit}`;
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
            selectedServerReadings.delete(key);
        } else {
            selectedServerReadings.set(key, samples[index].timestamp);
            targets[index].target.setAttribute('data-selected', '');
        }
    }
    selectReading(samples.findIndex(sample => sample.timestamp === selectedServerReadings.get(key)));
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
function queues(card,values){const colors=['#38bdf8','#c084fc','#34d399','#fbbf24','#fb7185'];for(const [key,types] of Object.entries(values)){card.append(cell('h3',key));const svg=document.createElementNS('http://www.w3.org/2000/svg','svg');svg.setAttribute('viewBox','0 0 600 100');svg.setAttribute('role','img');svg.setAttribute('aria-label',key+' queue activity');const series=Object.entries(types).map(([type,readings])=>[type,Object.values(readings).map(v=>Number(v??0))]);const max=Math.max(1,...series.flatMap(([,values])=>values));const legend=document.createElement('div');legend.className='legend';series.forEach(([type,readings],index)=>{const color=colors[index%colors.length];const line=document.createElementNS(svg.namespaceURI,'polyline');line.setAttribute('points',readings.map((v,i)=>`${i/Math.max(1,readings.length-1)*600},${95-v/max*90}`).join(' '));line.setAttribute('fill','none');line.setAttribute('stroke',color);line.setAttribute('stroke-width','2');svg.append(line);const label=cell('span',`${type}: ${readings.reduce((a,b)=>a+b,0)}`);label.style.borderColor=color;legend.append(label)});card.append(svg,legend)}}
function renderRows(card, values, headings = {}) {
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
            if (headings[column] && value?.title) td.title = value.title;
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

function render(data) {
    const focusedTrend = document.activeElement?.dataset.serverTrend;
    const timings = data.query_timing ?? {};
    main.replaceChildren();
    for (const [name, title] of Object.entries(names)) {
        const card = document.createElement('section');
        card.className = 'card';
        const heading = cell('h2', title);
        const queryTitle = name === 'cache'
            ? [queryTimingTitle(timings.cache_totals, 'Global '), queryTimingTitle(timings.cache, 'Key ')]
                .filter(Boolean).join(' ')
            : queryTimingTitle(timings[name]);
        if (queryTitle) heading.title = queryTitle;
        card.append(heading);
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
            queues(card, values);
        } else if (Array.isArray(values)) {
            renderRows(card, values);
        } else {
            for (const [key, types] of Object.entries(values)) {
                card.append(cell('h3', key));
                for (const [type, readings] of Object.entries(types)) {
                    const total = Object.values(readings).reduce((sum, value) => sum + Number(value ?? 0), 0);
                    card.append(cell('p', `${type}: ${total}`));
                }
            }
        }
        main.append(card);
    }
    const trends = new Map(Array.from(main.querySelectorAll('[data-server-trend]'),
        trend => [trend.dataset.serverTrend, trend]));
    for (const key of selectedServerReadings.keys()) {
        if (!trends.has(key)) selectedServerReadings.delete(key);
    }
    trends.get(focusedTrend)?.focus({preventScroll: true});
}
let activeRefresh;
async function refresh() {
    activeRefresh?.abort();
    const controller = new AbortController();
    activeRefresh = controller;
    try {
        const url = new URL(location.href);
        url.pathname = url.pathname.replace(/\/$/, '') + '/api/metrics';
        const options = new URLSearchParams(pageOptions);
        options.set('period', period.value);
        url.search = options.toString();
        const response = await fetch(url, {
            credentials: 'same-origin',
            headers: {Accept: 'application/json'},
            signal: controller.signal,
        });
        if (!response.ok) throw new Error(response.status === 403 ? 'Access denied' : 'Unable to load metrics');
        const data = await response.json();
        if (!controller.signal.aborted) {
            const focused = document.activeElement?.getAttribute('aria-label');
            render(data);
            if (focused) [...main.querySelectorAll('select')].find(select => select.getAttribute('aria-label') === focused)?.focus();
            status.textContent = 'Updated ' + new Date().toLocaleTimeString();
        }
    } catch (error) {
        if (!controller.signal.aborted) status.textContent = error.message;
    } finally {
        if (activeRefresh === controller) activeRefresh = undefined;
    }
}
period.addEventListener('change', () => updateOption('period', period.value));
document.querySelector('#refresh').addEventListener('click', refresh);
refresh();
setInterval(() => { if (!activeRefresh) refresh(); }, 5000);
