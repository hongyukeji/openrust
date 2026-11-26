
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
function serverTrend(card, label, readings, limit) {
    const values = Object.values(readings ?? {});
    if (!values.some(Number.isFinite) || !(limit > 0)) return;
    const namespace = 'http://www.w3.org/2000/svg';
    const svg = document.createElementNS(namespace, 'svg');
    svg.classList.add('server-trend');
    svg.setAttribute('viewBox', '0 0 600 40');
    svg.setAttribute('preserveAspectRatio', 'none');
    svg.setAttribute('role', 'img');
    svg.setAttribute('aria-label', label);
    const commands = [];
    values.forEach((value, index) => {
        if (!Number.isFinite(value)) return;
        const x = 2 + index / Math.max(1, values.length - 1) * 596;
        const y = 37 - Math.min(1, Math.max(0, value / limit)) * 34;
        const previous = Number.isFinite(values[index - 1]);
        commands.push(`${previous ? 'L' : 'M'} ${x} ${y}`);
        if (!previous && !Number.isFinite(values[index + 1])) {
            commands.push('h 0.01');
        }
    });
    const line = document.createElementNS(namespace, 'path');
    line.setAttribute('d', commands.join(' '));
    line.setAttribute('fill', 'none');
    line.setAttribute('stroke', 'currentColor');
    line.setAttribute('stroke-width', '3');
    line.setAttribute('stroke-linecap', 'round');
    line.setAttribute('vector-effect', 'non-scaling-stroke');
    svg.append(line);
    card.append(svg);
}
function servers(card, values, graphs) {
    for (const row of values) {
        let data;
        try {
            data = JSON.parse(row.value);
        } catch {
            card.append(cell('p', 'Invalid server snapshot'));
            continue;
        }
        card.append(cell('h3', data.name));
        const status = row.recently_reported ? 'Recently reported' : 'Stale';
        card.append(cell('small', `${status} · Last reported ${new Date(Number(row.timestamp) * 1000).toLocaleString()}`));
        const series = Object.hasOwn(graphs, row.key) ? graphs[row.key] : undefined;
        meter(card, `CPU · ${data.cpu}%`, Number(data.cpu), 100);
        serverTrend(card, `CPU history for ${data.name}`, series?.cpu, 100);
        meter(card, `Memory · ${data.memory_used} / ${data.memory_total} MiB`, Number(data.memory_used), Number(data.memory_total));
        serverTrend(card, `Memory history for ${data.name}`, series?.memory, Number(data.memory_total));
        for (const disk of data.storage ?? []) {
            meter(card, `${disk.directory} · ${disk.used} / ${disk.total} MiB`, Number(disk.used), Number(disk.total));
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
function render(data) {
    main.replaceChildren();
    for (const [name, title] of Object.entries(names)) {
        const card = document.createElement('section');
        card.className = 'card';
        card.append(cell('h2', title));
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
            servers(card, values, data.server_graphs ?? {});
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
setInterval(() => { if (!activeRefresh) refresh(); }, 10000);
