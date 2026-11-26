
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
function servers(card,values){for(const row of values){let data;try{data=JSON.parse(row.value)}catch{card.append(cell('p','Invalid server snapshot'));continue}card.append(cell('h3',data.name));meter(card,`CPU · ${data.cpu}%`,Number(data.cpu),100);meter(card,`Memory · ${data.memory_used} / ${data.memory_total} MiB`,Number(data.memory_used),Number(data.memory_total));for(const disk of data.storage??[])meter(card,`${disk.directory} · ${disk.used} / ${disk.total} MiB`,Number(disk.used),Number(disk.total));card.append(cell('small','Last reported '+new Date(Number(row.timestamp)*1000).toLocaleString()))}}
function queues(card,values){const colors=['#38bdf8','#c084fc','#34d399','#fbbf24','#fb7185'];for(const [key,types] of Object.entries(values)){card.append(cell('h3',key));const svg=document.createElementNS('http://www.w3.org/2000/svg','svg');svg.setAttribute('viewBox','0 0 600 100');svg.setAttribute('role','img');svg.setAttribute('aria-label',key+' queue activity');const series=Object.entries(types).map(([type,readings])=>[type,Object.values(readings).map(v=>Number(v??0))]);const max=Math.max(1,...series.flatMap(([,values])=>values));const legend=document.createElement('div');legend.className='legend';series.forEach(([type,readings],index)=>{const color=colors[index%colors.length];const line=document.createElementNS(svg.namespaceURI,'polyline');line.setAttribute('points',readings.map((v,i)=>`${i/Math.max(1,readings.length-1)*600},${95-v/max*90}`).join(' '));line.setAttribute('fill','none');line.setAttribute('stroke',color);line.setAttribute('stroke-width','2');svg.append(line);const label=cell('span',`${type}: ${readings.reduce((a,b)=>a+b,0)}`);label.style.borderColor=color;legend.append(label)});card.append(svg,legend)}}
function render(data){main.replaceChildren();for(const [name,title] of Object.entries(names)){const card=document.createElement('section');card.className='card';card.append(cell('h2',title));addFilter(card,name);const values=data[name];if(!values||Object.keys(values).length===0){const e=cell('div','No data for this period');e.className='empty';card.append(e)}else if(name==='servers'){servers(card,values)}else if(name==='queues'){queues(card,values)}else if(Array.isArray(values)){const box=document.createElement('div');box.className='rows';const table=document.createElement('table'),head=document.createElement('tr'),columns=Object.keys(values[0]);for(const k of columns)head.append(cell('th',k));table.append(head);for(const row of values){const tr=document.createElement('tr');for(const k of columns){const td=cell('td',row[k]);td.dataset.column=k;tr.append(td)}table.append(tr)}box.append(table);card.append(box)}else{for(const [key,types] of Object.entries(values)){card.append(cell('h3',key));for(const [type,readings] of Object.entries(types)){const total=Object.values(readings).reduce((sum,v)=>sum+Number(v??0),0);card.append(cell('p',`${type}: ${total}`))}}}main.append(card)}}
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
