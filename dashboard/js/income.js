// ── Helpers ───────────────────────────────────────────────────────────────────
function esc(s) {
  return String(s ?? '').replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');
}
const _eurFmt = new Intl.NumberFormat('el-GR', {minimumFractionDigits:2, maximumFractionDigits:2});
function fmtEur(n) { return '€' + _eurFmt.format(Number(n) || 0); }
function sym(cur) { return {EUR:'€',GBP:'£',USD:'$'}[cur] || (cur+' '); }
function fmtAmt(a, c) { return sym(c||'EUR') + _eurFmt.format(Number(a) || 0); }
function fmtDate(iso) {
  if (!iso) return '—';
  const [y,m,d] = iso.split('-');
  const M = ['Jan','Feb','Mar','Apr','May','Jun','Jul','Aug','Sep','Oct','Nov','Dec'];
  return `${parseInt(d)} ${M[parseInt(m)-1]} ${y}`;
}
function todayIso() { return new Date().toISOString().slice(0,10); }
function currentMonthIso() {
  const n = new Date();
  return `${n.getFullYear()}-${String(n.getMonth()+1).padStart(2,'0')}`;
}
function monthLabel(isoMonth) {
  const [y,m] = isoMonth.split('-').map(Number);
  const NAMES = ['JAN','FEB','MAR','APR','MAY','JUN','JUL','AUG','SEP','OCT','NOV','DEC'];
  return `${NAMES[m-1]} ${y}`;
}
function monthLongLabel(isoMonth) {
  const [y,m] = isoMonth.split('-').map(Number);
  const NAMES = ['January','February','March','April','May','June','July','August','September','October','November','December'];
  return `${NAMES[m-1]} ${y}`;
}
function prevMonthIso(isoMonth) {
  const [y,m] = isoMonth.split('-').map(Number);
  const d = new Date(y, m-2, 1);
  return `${d.getFullYear()}-${String(d.getMonth()+1).padStart(2,'0')}`;
}
function nextMonthIso(isoMonth) {
  const [y,m] = isoMonth.split('-').map(Number);
  const d = new Date(y, m, 1);
  return `${d.getFullYear()}-${String(d.getMonth()+1).padStart(2,'0')}`;
}
function monthStart(isoMonth) { return `${isoMonth}-01`; }
function monthEnd(isoMonth) {
  const [y,m] = isoMonth.split('-').map(Number);
  return `${isoMonth}-${String(new Date(y,m,0).getDate()).padStart(2,'0')}`;
}
function ordinal(n) {
  const s = ['th','st','nd','rd'], v = n%100;
  return n + (s[(v-20)%10] || s[v] || s[0]);
}
function toast(msg) {
  const t = document.getElementById('toast');
  t.textContent = msg; t.classList.add('show');
  setTimeout(() => t.classList.remove('show'), 4000);
}
function showErr(id, msg) {
  const el = document.getElementById(id);
  if (!el) return;
  el.textContent = msg; el.style.display = msg ? '' : 'none';
}
function setBtn(id, disabled, txt) {
  const b = document.getElementById(id);
  if (!b) return;
  b.disabled = disabled; b.textContent = txt;
}
function val(id) { return (document.getElementById(id) || {}).value || ''; }
function setVal(id, v) { const el = document.getElementById(id); if (el) el.value = v ?? ''; }

// ── State ─────────────────────────────────────────────────────────────────────
let _currentView  = localStorage.getItem('income.view')  || 'monthly';
let _currentMonth = localStorage.getItem('income.month') || currentMonthIso();
let _allEvents    = [];  // all payment events (broad fetch)
let _invoices     = [];
let _contracts    = [];
let _invoiceFilter = 'all';
let _editId = 0, _editType = '';
let _pe = {};  // payment event modal state

// ── View toggle ───────────────────────────────────────────────────────────────
function switchView(v) {
  _currentView = v;
  localStorage.setItem('income.view', v);
  document.getElementById('view-monthly').style.display = v === 'monthly' ? '' : 'none';
  document.getElementById('view-yearly').style.display  = v === 'yearly'  ? '' : 'none';
  document.getElementById('vt-monthly').classList.toggle('active', v === 'monthly');
  document.getElementById('vt-yearly').classList.toggle('active', v === 'yearly');
  if (v === 'yearly') loadYearly();
}

document.getElementById('vt-monthly').addEventListener('click', () => switchView('monthly'));
document.getElementById('vt-yearly').addEventListener('click',  () => switchView('yearly'));

// ── Month navigation ──────────────────────────────────────────────────────────
function updateMonthLabel() {
  document.getElementById('month-label').textContent = monthLabel(_currentMonth);
  const picker = document.getElementById('month-picker');
  if (picker) picker.value = _currentMonth;
}

function setMonth(isoMonth) {
  _currentMonth = isoMonth;
  localStorage.setItem('income.month', _currentMonth);
  updateMonthLabel();
  loadAll();
}

document.getElementById('btn-prev-month').addEventListener('click', () => setMonth(prevMonthIso(_currentMonth)));
document.getElementById('btn-next-month').addEventListener('click', () => setMonth(nextMonthIso(_currentMonth)));

document.getElementById('month-label').addEventListener('click', () => {
  const picker = document.getElementById('month-picker');
  if (picker && picker.showPicker) { try { picker.showPicker(); } catch(e) {} }
});

const _monthPicker = document.getElementById('month-picker');
if (_monthPicker) {
  _monthPicker.addEventListener('change', e => {
    if (e.target.value) setMonth(e.target.value);
  });
}

// ── Hero add dropdown ─────────────────────────────────────────────────────────
document.getElementById('btn-hero-add').addEventListener('click', e => {
  e.stopPropagation();
  document.getElementById('hero-add-menu').classList.toggle('open');
});
document.addEventListener('click', () => {
  document.getElementById('hero-add-menu').classList.remove('open');
});
document.getElementById('hero-add-menu').addEventListener('click', () => {
  document.getElementById('hero-add-menu').classList.remove('open');
});

// ── Load all ──────────────────────────────────────────────────────────────────
async function loadAll() {
  await loadPaymentEvents();
  await Promise.all([loadSummary(), loadSalaries(), loadRentals(), loadInvoices(), loadOther()]);
  await loadContracts(); // must run after loadInvoices so _invoices is populated for contract filtering
}

// ── Summary ───────────────────────────────────────────────────────────────────
async function loadSummary() {
  try {
    const d = await fetch(`/api/income/summary?year=${_currentMonth.split('-')[0]}&month=${_currentMonth}`).then(r=>r.json());
    if (!d.ok) throw new Error(d.error || 'Failed');
    const mon = d.summary.month || {};
    renderHero(mon);
    renderBreakdown(mon.by_source);
  } catch(e) {
    document.getElementById('mon-gross').textContent = '—';
    document.getElementById('mon-net').textContent = esc(e.message);
  }
}

function renderHero(mon) {
  const {gross=0, net=0, withholding=0, pending_gross=0} = mon;
  document.getElementById('mon-gross').textContent = gross > 0.005 ? fmtEur(gross) : '—';
  document.getElementById('mon-net').textContent   = gross > 0.005 ? fmtEur(net) + ' net' : '—';
  const withEl = document.getElementById('mon-withholding');
  if (withholding > 0.005) {
    withEl.textContent = 'after ' + fmtEur(withholding) + ' withholding';
    withEl.style.display = '';
  } else { withEl.style.display = 'none'; }
  const pendEl = document.getElementById('mon-pending');
  if (pending_gross > 0.005) {
    pendEl.textContent = '+' + fmtEur(pending_gross) + ' expected';
    pendEl.style.display = '';
  } else { pendEl.style.display = 'none'; }
}

const SRC_COLORS = {
  invoices:  'rgba(58,138,170,0.9)',
  rentals:   'rgba(212,168,56,0.9)',
  salaries:  'rgba(147,112,219,0.9)',
  contracts: 'rgba(96,165,250,0.9)',
  other:     'rgba(156,163,175,0.9)',
};

function renderBreakdown(by_source) {
  const bd = document.getElementById('hero-breakdown');
  const sources = [
    {key:'invoices',  label:'Invoices'},
    {key:'rentals',   label:'Rentals'},
    {key:'salaries',  label:'Salaries'},
    {key:'contracts', label:'Contracts'},
    {key:'other',     label:'Other'},
  ];
  const chips = sources.map(({key, label}) => {
    const g = ((by_source || {})[key] || {}).gross || 0;
    if (g < 0.005) return '';
    const color = SRC_COLORS[key] || 'rgba(255,255,255,0.5)';
    return `<div class="hero-chip"><div class="hero-chip-dot" style="background:${color}"></div>${esc(label)} ${fmtEur(g)}</div>`;
  }).filter(Boolean).join('');
  bd.innerHTML = chips || `<div class="hero-chip" style="color:rgba(255,255,255,.22)">No income this month</div>`;
}

// ── Payment events ────────────────────────────────────────────────────────────
async function loadPaymentEvents() {
  try {
    const d = await fetch('/api/income/payment-events?start=2020-01-01&end=2099-12-31').then(r=>r.json());
    _allEvents = (d.events || d.payment_events || []);
  } catch(e) {
    _allEvents = [];
  }
}

function getMonthEvent(sourceType, sourceId) {
  return _allEvents.find(e =>
    e.source_type === sourceType &&
    e.source_id   === sourceId   &&
    (e.paid_date_month || (e.paid_date || '').slice(0,7)) === _currentMonth
  );
}

function getInvoicePaidTotal(invoiceId) {
  return _allEvents
    .filter(e => e.source_type === 'invoice' && e.source_id === invoiceId)
    .reduce((s, e) => s + (Number(e.amount) || 0), 0);
}

function buildMonthEventSet() {
  const set = new Set();
  for (const e of _allEvents) {
    const eventMonth = e.paid_date_month || (e.paid_date || '').slice(0, 7);
    if (eventMonth === _currentMonth) {
      set.add(`${e.source_type}:${e.source_id}`);
    }
  }
  return set;
}

// ── Pill HTML ──────────────────────────────────────────────────────────────────
function pillHtml(cls, text, clickable, dataset) {
  const ds = dataset ? Object.entries(dataset).map(([k,v]) => `data-${k}="${esc(v)}"`).join(' ') : '';
  const cc = clickable ? ' s-clickable' : '';
  return `<span class="spill ${cls}${cc}" ${ds}>${esc(text)}</span>`;
}

function rentalPillInfo(rentalId) {
  const ev = getMonthEvent('rental', rentalId);
  if (!ev) return {cls:'s-none', text:'—'};
  if (ev.status === 'received') return {cls:'s-received', text:'PAID'};
  return {cls:'s-expected', text:'EXPECTED'};
}

function salaryPillInfo(salaryId) {
  const ev = getMonthEvent('salary', salaryId);
  if (!ev) return {cls:'s-none', text:'—'};
  if (ev.status === 'received') return {cls:'s-received', text:'PAID'};
  return {cls:'s-expected', text:'EXPECTED'};
}

function contractPillInfo(c) {
  const inv = Number(c.invoiced_total) || 0;
  const paid = Number(c.paid_total) || 0;
  const status = (c.status || '').toLowerCase();
  if (status === 'cancelled') return {cls:'s-cancelled', text:'CANCELLED'};
  if (status === 'completed') {
    return paid >= inv && inv > 0 ? {cls:'s-fully-paid', text:'FULLY PAID'} : {cls:'s-completed', text:'COMPLETED'};
  }
  if (inv > 0) {
    if (paid >= inv) return {cls:'s-fully-paid', text:'FULLY PAID'};
    if (paid > 0)    return {cls:'s-partial',    text:'PARTIAL'};
    return {cls:'s-invoiced', text:'INVOICED'};
  }
  return {cls:'s-active', text:'ACTIVE'};
}

function invoicePillInfo(inv) {
  const paid = getInvoicePaidTotal(inv.id);
  const amount = Number(inv.amount) || 0;
  const status = (inv.status || '').toLowerCase();
  const today = todayIso();
  if (paid >= amount && amount > 0) return {cls:'s-paid', text:'PAID'};
  if (paid > 0 && paid < amount)    return {cls:'s-partial', text:'PARTIAL'};
  if (inv.due_date && inv.due_date < today && paid === 0) return {cls:'s-overdue', text:'OVERDUE'};
  if (status === 'sent')      return {cls:'s-sent',      text:'SENT'};
  if (status === 'cancelled') return {cls:'s-cancelled', text:'CANCELLED'};
  if (status === 'void')      return {cls:'s-void',      text:'VOID'};
  return {cls:'s-draft', text:'DRAFT'};
}

// ── Salaries ──────────────────────────────────────────────────────────────────
async function loadSalaries() {
  const el = document.getElementById('salaries-container');
  try {
    const d = await fetch('/api/income/salaries').then(r=>r.json());
    if (!d.ok) throw new Error(d.error || 'Failed');
    let list = d.salaries ?? [];
    if (_currentView === 'monthly') {
      const mSet = buildMonthEventSet();
      list = list.filter(s => mSet.has(`salary:${s.id}`));
    }
    if (!list.length) {
      el.innerHTML = `<div class="income-empty">${
        _currentView === 'monthly'
          ? `No salary activity in ${monthLabel(_currentMonth)}.`
          : 'No salaries recorded yet.'
      }</div>`;
      return;
    }
    el.innerHTML = `<div class="income-list">${list.map(renderSalaryRow).join('')}</div>`;
    wireSectionRows(el, 'salary', list);
  } catch(e) {
    el.innerHTML = `<div class="income-empty" style="color:#c47a7a">${esc(e.message)}</div>`;
  }
}

function renderSalaryRow(s) {
  const {cls, text} = salaryPillInfo(s.id);
  const title = esc(s.display_name || s.employer || '—');
  const pill = pillHtml(cls, text, true, {pe:'salary', peid: String(s.id), peamt: String(s.gross_monthly)});
  return `<div class="income-row" data-edit-type="salary" data-edit-id="${s.id}">
    <div class="irow-body">
      <div class="irow-title">${title}</div>
      <div class="irow-sub">${esc(s.role || '—')}</div>
      <div class="irow-meta">Pays on the ${ordinal(s.pay_day)}${s.start_date?' · from '+fmtDate(s.start_date):''}</div>
    </div>
    <div class="irow-right">${pill}<div class="irow-amount">${fmtAmt(s.gross_monthly,'EUR')}</div></div>
  </div>`;
}

// ── Rentals ───────────────────────────────────────────────────────────────────
async function loadRentals() {
  const el = document.getElementById('rentals-container');
  try {
    const d = await fetch('/api/income/rentals').then(r=>r.json());
    if (!d.ok) throw new Error(d.error || 'Failed');
    let list = d.rentals ?? [];
    if (_currentView === 'monthly') {
      const mSet = buildMonthEventSet();
      list = list.filter(r => mSet.has(`rental:${r.id}`));
    }
    if (!list.length) {
      el.innerHTML = `<div class="income-empty">${
        _currentView === 'monthly'
          ? `No rental activity in ${monthLabel(_currentMonth)}.`
          : 'No rental income recorded yet.'
      }</div>`;
      return;
    }
    el.innerHTML = `<div class="income-list">${list.map(renderRentalRow).join('')}</div>`;
    wireSectionRows(el, 'rental', list);
  } catch(e) {
    el.innerHTML = `<div class="income-empty" style="color:#c47a7a">${esc(e.message)}</div>`;
  }
}

function renderRentalRow(r) {
  const {cls, text} = rentalPillInfo(r.id);
  const title = esc(r.display_name || r.property_name || '—');
  const pill = pillHtml(cls, text, true, {pe:'rental', peid: String(r.id), peamt: String(r.monthly_rent)});
  return `<div class="income-row" data-edit-type="rental" data-edit-id="${r.id}">
    <div class="irow-body">
      <div class="irow-title">${title}</div>
      <div class="irow-sub">${esc(r.tenant_name || 'No tenant')}</div>
      <div class="irow-meta">Due on the ${ordinal(r.payment_day)}${r.contract_start?' · from '+fmtDate(r.contract_start):''}</div>
    </div>
    <div class="irow-right">${pill}<div class="irow-amount">${fmtAmt(r.monthly_rent, r.currency||'EUR')}</div></div>
  </div>`;
}

// ── Contracts ─────────────────────────────────────────────────────────────────
async function loadContracts() {
  const el = document.getElementById('contracts-container');
  try {
    const d = await fetch('/api/income/contracts').then(r=>r.json());
    if (!d.ok) throw new Error(d.error || 'Failed');
    _contracts = d.contracts ?? [];
    let list = _contracts;
    if (_currentView === 'monthly') {
      const mSet = buildMonthEventSet();
      const contractIdsThisMonth = new Set(
        _invoices
          .filter(i => mSet.has(`invoice:${i.id}`))
          .map(i => i.contract_id)
          .filter(id => id != null)
      );
      list = _contracts.filter(c => contractIdsThisMonth.has(c.id));
    }
    if (!list.length) {
      el.innerHTML = `<div class="income-empty">${
        _currentView === 'monthly'
          ? `No contract activity in ${monthLabel(_currentMonth)}.`
          : 'No contracts recorded yet.'
      }</div>`;
      return;
    }
    el.innerHTML = `<div class="income-list">${list.map(renderContractRow).join('')}</div>`;
    wireSectionRows(el, 'contract', list);
  } catch(e) {
    el.innerHTML = `<div class="income-empty" style="color:#c47a7a">${esc(e.message)}</div>`;
  }
}

function renderContractRow(c) {
  const {cls, text} = contractPillInfo(c);
  const TYPE = {fixed:'Fixed price',hourly:'Hourly',retainer:'Retainer',milestone:'Milestone'};
  const val = c.total_value != null ? fmtAmt(c.total_value, c.currency) + ' total'
            : c.monthly_value != null ? fmtAmt(c.monthly_value, c.currency) + '/mo' : '—';
  const title = esc(c.display_name || c.contract_name || '—');
  return `<div class="income-row" data-edit-type="contract" data-edit-id="${c.id}">
    <div class="irow-body">
      <div class="irow-title">${title}</div>
      <div class="irow-sub">${esc(c.client_name || '—')}</div>
      <div class="irow-meta">${esc(TYPE[c.contract_type]||c.contract_type||'Contract')}${c.start_date?' · from '+fmtDate(c.start_date):''}</div>
    </div>
    <div class="irow-right">${pillHtml(cls, text, false)}<div class="irow-amount">${val}</div></div>
  </div>`;
}

// ── Invoices ──────────────────────────────────────────────────────────────────
async function loadInvoices() {
  const el = document.getElementById('invoices-container');
  try {
    const d = await fetch('/api/income/invoices').then(r=>r.json());
    if (!d.ok) throw new Error(d.error || 'Failed');
    _invoices = d.invoices ?? [];
    renderInvoices();
  } catch(e) {
    el.innerHTML = `<div class="income-empty" style="color:#c47a7a">${esc(e.message)}</div>`;
  }
}

function renderInvoices() {
  const el = document.getElementById('invoices-container');
  let list = _invoices;
  if (_currentView === 'monthly') {
    const mSet = buildMonthEventSet();
    list = list.filter(inv => mSet.has(`invoice:${inv.id}`));
  }
  if (_invoiceFilter !== 'all') {
    list = list.filter(inv => {
      const {cls} = invoicePillInfo(inv);
      if (_invoiceFilter === 'paid')    return cls === 's-paid';
      if (_invoiceFilter === 'overdue') return cls === 's-overdue';
      if (_invoiceFilter === 'sent')    return cls === 's-sent';
      return true;
    });
  }
  if (!list.length) {
    const emptyMsg = _currentView === 'monthly'
      ? `No invoice activity in ${monthLabel(_currentMonth)}.`
      : _invoiceFilter === 'all' ? 'No invoices recorded yet.' : `No ${_invoiceFilter} invoices.`;
    el.innerHTML = `<div class="income-empty">${emptyMsg}</div>`;
    return;
  }
  el.innerHTML = `<div class="income-list">${list.map(renderInvoiceRow).join('')}</div>`;
  wireSectionRows(el, 'invoice', list);
}

function renderInvoiceRow(inv) {
  const {cls, text} = invoicePillInfo(inv);
  const paidTotal = getInvoicePaidTotal(inv.id);
  const title = esc(inv.display_name || inv.invoice_number || inv.client_name || '—');
  let sub = esc(inv.client_name || '—');
  if (inv.project_code) sub += ' · ' + esc(inv.project_code);
  if (cls === 's-partial') sub += ` · ${fmtEur(paidTotal)} of ${fmtEur(inv.amount)}`;
  return `<div class="income-row" data-edit-type="invoice" data-edit-id="${inv.id}">
    <div class="irow-body">
      <div class="irow-title">${title}</div>
      <div class="irow-sub">${sub}</div>
      <div class="irow-meta">${inv.issue_date?'Issued '+fmtDate(inv.issue_date):''}${inv.due_date?' · due '+fmtDate(inv.due_date):''}</div>
    </div>
    <div class="irow-right">${pillHtml(cls, text, true)}<div class="irow-amount">${fmtAmt(inv.amount, inv.currency)}</div></div>
  </div>`;
}

// ── Other income ──────────────────────────────────────────────────────────────
async function loadOther() {
  const el = document.getElementById('other-container');
  try {
    const d = await fetch('/api/income/other').then(r=>r.json());
    if (!d.ok) throw new Error(d.error || 'Failed');
    let list = d.other ?? [];
    if (_currentView === 'monthly') {
      const mSet = buildMonthEventSet();
      list = list.filter(o => mSet.has(`other_income:${o.id}`));
    }
    if (!list.length) {
      el.innerHTML = `<div class="income-empty">${
        _currentView === 'monthly'
          ? `No other income activity in ${monthLabel(_currentMonth)}.`
          : 'No other income recorded yet.'
      }</div>`;
      return;
    }
    el.innerHTML = `<div class="income-list">${list.map(renderOtherRow).join('')}</div>`;
    wireSectionRows(el, 'other', list);
  } catch(e) {
    el.innerHTML = `<div class="income-empty" style="color:#c47a7a">${esc(e.message)}</div>`;
  }
}

function renderOtherRow(o) {
  const status = (o.status || 'pending').toLowerCase();
  const clsMap = {received:'s-received', pending:'s-expected', cancelled:'s-cancelled'};
  const txtMap = {received:'RECEIVED', pending:'EXPECTED', cancelled:'CANCELLED'};
  const cls = clsMap[status] || 's-none';
  const txt = txtMap[status] || status.toUpperCase();
  const title = esc(o.display_name || o.description || '—');
  return `<div class="income-row" data-edit-type="other" data-edit-id="${o.id}">
    <div class="irow-body">
      <div class="irow-title">${title}</div>
      <div class="irow-sub">${o.recurring ? esc(o.cadence||'recurring') : 'One-time'}</div>
      <div class="irow-meta">${fmtDate(o.date_received || o.expected_date)}</div>
    </div>
    <div class="irow-right">${pillHtml(cls, txt, false)}<div class="irow-amount">${fmtAmt(o.amount, o.currency)}</div></div>
  </div>`;
}

// ── Wire section rows (row click → edit, pill click → payment event) ──────────
function wireSectionRows(container, type, list) {
  container.querySelectorAll('.income-row').forEach((row, idx) => {
    const record = list[idx];
    if (!record) return;

    row.addEventListener('click', e => {
      if (e.target.closest('.spill[data-pe]')) return; // rental/salary payment-event pills only
      openEditModal(type, record);
    });

    row.querySelectorAll('.spill[data-pe]').forEach(pill => {
      pill.addEventListener('click', e => {
        e.stopPropagation();
        const sourceType = pill.dataset.pe;
        const sourceId   = parseInt(pill.dataset.peid, 10);
        const defAmt     = parseFloat(pill.dataset.peamt) || 0;
        const displayName = record.display_name || record.property_name || record.employer || record.description || '—';
        openPaymentEvent(sourceType, sourceId, displayName, defAmt);
      });
    });
  });
}

// ── Invoice filter ────────────────────────────────────────────────────────────
document.querySelectorAll('.filter-pill').forEach(pill => {
  pill.addEventListener('click', () => {
    document.querySelectorAll('.filter-pill').forEach(p => p.classList.remove('active'));
    pill.classList.add('active');
    _invoiceFilter = pill.dataset.filter;
    renderInvoices();
  });
});

// ── Edit modals ───────────────────────────────────────────────────────────────
const EDIT_DIALOGS = {
  rental:   'modal-edit-rental',
  salary:   'modal-edit-salary',
  contract: 'modal-edit-contract',
  invoice:  'modal-edit-invoice',
  other:    'modal-edit-other',
};
const URL_MAP = {salary:'salaries', rental:'rentals', contract:'contracts', invoice:'invoices', other:'other'};

function openEditModal(type, record) {
  _editType = type; _editId = record.id;
  const dlgId = EDIT_DIALOGS[type];
  if (!dlgId) return;
  if (type === 'rental')   openEditRental(record);
  if (type === 'salary')   openEditSalary(record);
  if (type === 'contract') openEditContract(record);
  if (type === 'invoice')  openEditInvoice(record);
  if (type === 'other')    openEditOther(record);
  resetDelConfirm(type);
  document.getElementById(dlgId).showModal();
}

function resetDelConfirm(type) {
  const prefix = {rental:'er',salary:'es',contract:'ec',invoice:'ei',other:'eo'}[type];
  if (!prefix) return;
  const delBtn = document.getElementById(prefix+'-del');
  const confirmEl = document.getElementById(prefix+'-del-confirm');
  if (delBtn) delBtn.style.display = '';
  if (confirmEl) { confirmEl.style.display = 'none'; confirmEl.style.alignItems = ''; }
}

function openEditRental(r) {
  setVal('er-display-name', r.display_name || '');
  setVal('er-prop',        r.property_name);
  setVal('er-tenant',      r.tenant_name);
  setVal('er-amount',      r.monthly_rent);
  setVal('er-paymentday',  r.payment_day);
  setVal('er-start',       r.contract_start);
  setVal('er-end',         r.contract_end || '');
  setVal('er-status',      r.status || 'active');
  setVal('er-notes',       r.notes || '');
  showErr('er-error', '');
  setBtn('er-save', false, 'Save');
}

function openEditSalary(s) {
  setVal('es-display-name', s.display_name || '');
  setVal('es-employer',     s.employer);
  setVal('es-role',         s.role || '');
  setVal('es-gross',        s.gross_monthly);
  setVal('es-payday',       s.pay_day);
  setVal('es-start',        s.start_date || '');
  setVal('es-end',          s.end_date || '');
  setVal('es-status',       s.status || 'active');
  setVal('es-notes',        s.notes || '');
  showErr('es-error', '');
  setBtn('es-save', false, 'Save');
}

function openEditContract(c) {
  setVal('ec-display-name', c.display_name || '');
  setVal('ec-name',         c.contract_name);
  setVal('ec-client',       c.client_name);
  setVal('ec-type',         c.contract_type || 'retainer');
  setVal('ec-total',        c.total_value != null ? c.total_value : '');
  setVal('ec-monthly',      c.monthly_value != null ? c.monthly_value : '');
  setVal('ec-projcode',     c.project_code || '');
  setVal('ec-start',        c.start_date || '');
  setVal('ec-end',          c.end_date || '');
  setVal('ec-status',       c.status || 'active');
  setVal('ec-notes',        c.notes || '');
  const invT = Number(c.invoiced_total)||0, paidT = Number(c.paid_total)||0, totV = Number(c.total_value)||0;
  const sumEl = document.getElementById('ec-summary');
  if (sumEl) {
    if (invT > 0 || paidT > 0) {
      sumEl.textContent = `Invoiced: ${fmtEur(invT)}${totV?` of ${fmtEur(totV)}`:''} · Paid: ${fmtEur(paidT)}`;
      sumEl.style.display = '';
    } else { sumEl.style.display = 'none'; }
  }
  showErr('ec-error', '');
  setBtn('ec-save', false, 'Save');
}

function openEditInvoice(inv) {
  setVal('ei-display-name', inv.display_name || '');
  setVal('ei-client',       inv.client_name);
  setVal('ei-invnum',       inv.invoice_number || '');
  setVal('ei-issue',        inv.issue_date || '');
  setVal('ei-due',          inv.due_date || '');
  setVal('ei-amount',       inv.amount);
  setVal('ei-amount-net',   inv.amount_net != null ? inv.amount_net : '');
  setVal('ei-wht',          inv.withholding_tax != null ? inv.withholding_tax : '');
  setVal('ei-status',       inv.status && inv.status !== 'paid' ? inv.status : 'sent');
  setVal('ei-projcode',     inv.project_code || '');
  setVal('ei-notes',        inv.notes || '');
  // Contract dropdown
  const sel = document.getElementById('ei-contract');
  sel.innerHTML = '<option value="">— None —</option>';
  _contracts.forEach(c => {
    const o = document.createElement('option');
    o.value = c.id; o.textContent = `${c.client_name} — ${c.contract_name}`;
    sel.appendChild(o);
  });
  if (inv.contract_id) sel.value = String(inv.contract_id);
  // File link
  const fileRow = document.getElementById('ei-file-row');
  const fileLink = document.getElementById('ei-file-link');
  if (inv.attached_file_path) {
    fileLink.href = inv.attached_file_path;
    fileLink.textContent = inv.attached_file_path.split('/').pop() || 'View file';
    if (fileRow) fileRow.style.display = '';
  } else {
    if (fileRow) fileRow.style.display = 'none';
  }
  showErr('ei-error', '');
  setBtn('ei-save', false, 'Save');
  _eiPaymentsChanged = false;
  loadInvoicePayments(inv);
}

function openEditOther(o) {
  setVal('eo-display-name', o.display_name || '');
  setVal('eo-desc',         o.description);
  setVal('eo-amount',       o.amount);
  setVal('eo-currency',     o.currency || 'EUR');
  setVal('eo-expected',     o.date_received || o.expected_date || '');
  setVal('eo-status',       o.status || 'pending');
  setVal('eo-notes',        o.notes || '');
  showErr('eo-error', '');
  setBtn('eo-save', false, 'Save');
}

// ── Edit save handlers ────────────────────────────────────────────────────────
async function saveEdit(prefix, urlKey, payload, errorId, saveId, dialogId) {
  if (!payload) return;
  setBtn(saveId, true, 'Saving…');
  try {
    const d = await fetch(`/api/income/${urlKey}/${_editId}`, {
      method: 'PATCH', headers: {'Content-Type':'application/json'},
      body: JSON.stringify(payload),
    }).then(r=>r.json());
    if (!d.ok) throw new Error(d.error || 'Save failed');
    document.getElementById(dialogId).close();
    await loadAll();
    toast('Saved.');
  } catch(e) { showErr(errorId, e.message); }
  finally { setBtn(saveId, false, 'Save'); }
}

document.getElementById('er-save').addEventListener('click', () => {
  const property_name = val('er-prop').trim();
  const monthly_rent  = parseFloat(val('er-amount'));
  const payment_day   = parseInt(val('er-paymentday'), 10);
  if (!property_name)                                  return showErr('er-error','Property name is required.');
  if (isNaN(monthly_rent) || monthly_rent <= 0)        return showErr('er-error','Enter a valid amount.');
  if (isNaN(payment_day) || payment_day<1||payment_day>31) return showErr('er-error','Payment day must be 1–31.');
  saveEdit('er','rentals',{
    display_name:    val('er-display-name').trim()||null,
    property_name, monthly_rent, payment_day,
    tenant_name:     val('er-tenant').trim()||null,
    contract_start:  val('er-start')||null,
    contract_end:    val('er-end')||null,
    status:          val('er-status'),
    notes:           val('er-notes').trim()||null,
  },'er-error','er-save','modal-edit-rental');
});

document.getElementById('es-save').addEventListener('click', () => {
  const employer      = val('es-employer').trim();
  const gross_monthly = parseFloat(val('es-gross'));
  const pay_day       = parseInt(val('es-payday'), 10);
  if (!employer)                                       return showErr('es-error','Employer is required.');
  if (isNaN(gross_monthly)||gross_monthly<=0)          return showErr('es-error','Enter a valid amount.');
  if (isNaN(pay_day)||pay_day<1||pay_day>31)           return showErr('es-error','Pay day must be 1–31.');
  saveEdit('es','salaries',{
    display_name: val('es-display-name').trim()||null,
    employer, gross_monthly, pay_day,
    role:       val('es-role').trim()||null,
    start_date: val('es-start')||null,
    end_date:   val('es-end')||null,
    status:     val('es-status'),
    notes:      val('es-notes').trim()||null,
  },'es-error','es-save','modal-edit-salary');
});

document.getElementById('ec-save').addEventListener('click', () => {
  const contract_name = val('ec-name').trim();
  const client_name   = val('ec-client').trim();
  if (!contract_name) return showErr('ec-error','Contract name is required.');
  if (!client_name)   return showErr('ec-error','Client name is required.');
  const total_value   = parseFloat(val('ec-total'));
  const monthly_value = parseFloat(val('ec-monthly'));
  saveEdit('ec','contracts',{
    display_name:  val('ec-display-name').trim()||null,
    contract_name, client_name,
    contract_type: val('ec-type'),
    total_value:   isNaN(total_value)   ? null : total_value,
    monthly_value: isNaN(monthly_value) ? null : monthly_value,
    project_code:  val('ec-projcode').trim()||null,
    start_date:    val('ec-start')||null,
    end_date:      val('ec-end')||null,
    status:        val('ec-status'),
    notes:         val('ec-notes').trim()||null,
  },'ec-error','ec-save','modal-edit-contract');
});

document.getElementById('ei-save').addEventListener('click', () => {
  const client_name = val('ei-client').trim();
  const amount      = parseFloat(val('ei-amount'));
  if (!client_name)                      return showErr('ei-error','Client name is required.');
  if (isNaN(amount)||amount<=0)          return showErr('ei-error','Enter a valid amount.');
  const amount_net      = parseFloat(val('ei-amount-net'));
  const withholding_tax = parseFloat(val('ei-wht'));
  const contract_id     = parseInt(val('ei-contract'), 10) || null;
  saveEdit('ei','invoices',{
    display_name:   val('ei-display-name').trim()||null,
    client_name, amount,
    invoice_number: val('ei-invnum').trim()||null,
    issue_date:     val('ei-issue')||null,
    due_date:       val('ei-due')||null,
    amount_net:     isNaN(amount_net)      ? null : amount_net,
    withholding_tax:isNaN(withholding_tax) ? null : withholding_tax,
    status:         val('ei-status'),
    project_code:   val('ei-projcode').trim()||null,
    contract_id,
    notes:          val('ei-notes').trim()||null,
  },'ei-error','ei-save','modal-edit-invoice');
});

document.getElementById('eo-save').addEventListener('click', () => {
  const description = val('eo-desc').trim();
  const amount      = parseFloat(val('eo-amount'));
  if (!description)              return showErr('eo-error','Description is required.');
  if (isNaN(amount)||amount<=0)  return showErr('eo-error','Enter a valid amount.');
  saveEdit('eo','other',{
    display_name:  val('eo-display-name').trim()||null,
    description, amount,
    currency:      val('eo-currency'),
    expected_date: val('eo-expected')||null,
    status:        val('eo-status'),
    notes:         val('eo-notes').trim()||null,
  },'eo-error','eo-save','modal-edit-other');
});

// ── Inline delete (2-step) ────────────────────────────────────────────────────
function startDel(prefix) {
  document.getElementById(prefix+'-del').style.display = 'none';
  const c = document.getElementById(prefix+'-del-confirm');
  c.style.display = 'flex'; c.style.alignItems = 'center';
}
function cancelDel(prefix) {
  document.getElementById(prefix+'-del').style.display = '';
  document.getElementById(prefix+'-del-confirm').style.display = 'none';
}

const DEL_MAP = {er:'rental',es:'salary',ec:'contract',ei:'invoice',eo:'other'};

['er','es','ec','ei','eo'].forEach(prefix => {
  const delBtn = document.getElementById(prefix+'-del');
  const yesBtn = document.getElementById(prefix+'-del-yes');
  if (delBtn) delBtn.addEventListener('click', () => startDel(prefix));
  if (yesBtn) yesBtn.addEventListener('click', async () => {
    const type = DEL_MAP[prefix];
    const urlKey = URL_MAP[type];
    const dialogId = EDIT_DIALOGS[type];
    yesBtn.disabled = true; yesBtn.textContent = 'Deleting…';
    try {
      const d = await fetch(`/api/income/${urlKey}/${_editId}`, {method:'DELETE'}).then(r=>r.json());
      if (!d.ok) throw new Error(d.error||'Delete failed');
      document.getElementById(dialogId).close();
      await loadAll();
      toast('Deleted.');
    } catch(e) {
      yesBtn.disabled = false; yesBtn.textContent = 'Yes, delete';
      toast('Error: '+e.message);
    }
  });
});

// ── Payment event modal ───────────────────────────────────────────────────────
function openPaymentEvent(sourceType, sourceId, displayName, defaultAmount) {
  _pe = {sourceType, sourceId, displayName, defaultAmount, eventId: null, status: 'expected'};
  const ev = getMonthEvent(sourceType, sourceId);
  if (ev) {
    _pe.eventId = ev.id;
    _pe.status  = ev.status || 'received';
    setVal('pe-date',   ev.paid_date || todayIso());
    setVal('pe-amount', ev.amount);
    setVal('pe-note',   ev.confirmation_note || '');
  } else {
    setVal('pe-date',   todayIso());
    setVal('pe-amount', defaultAmount);
    setVal('pe-note',   '');
  }
  setPeStatus(_pe.status);
  document.getElementById('pe-title').textContent = `${monthLongLabel(_currentMonth)} — ${displayName}`;
  showErr('pe-error', '');
  setBtn('pe-save', false, 'Save');
  document.getElementById('modal-payment-event').showModal();
}

function setPeStatus(status) {
  _pe.status = status;
  const btnR = document.getElementById('pe-btn-received');
  const btnE = document.getElementById('pe-btn-expected');
  btnR.className = 'pe-toggle-btn' + (status === 'received' ? ' state-received' : '');
  btnE.className = 'pe-toggle-btn' + (status === 'expected' ? ' state-expected' : '');
}

document.getElementById('pe-btn-received').addEventListener('click', () => setPeStatus('received'));
document.getElementById('pe-btn-expected').addEventListener('click', () => setPeStatus('expected'));

document.getElementById('pe-save').addEventListener('click', async () => {
  const amount    = parseFloat(val('pe-amount'));
  const paid_date = val('pe-date') || todayIso();
  const confirmation_note = val('pe-note').trim() || null;
  if (isNaN(amount)||amount<=0) return showErr('pe-error','Enter a valid amount.');
  setBtn('pe-save', true, 'Saving…');
  try {
    if (_pe.eventId) {
      const d = await fetch(`/api/income/payment-events/${_pe.eventId}`, {
        method:'PATCH', headers:{'Content-Type':'application/json'},
        body: JSON.stringify({status:_pe.status, paid_date, amount, confirmation_note}),
      }).then(r=>r.json());
      if (!d.ok) throw new Error(d.error||'Failed');
    } else if (_pe.status === 'received') {
      const d = await fetch('/api/income/payments', {
        method:'POST', headers:{'Content-Type':'application/json'},
        body: JSON.stringify({source_type:_pe.sourceType, source_id:_pe.sourceId, amount, paid_date, note:confirmation_note}),
      }).then(r=>r.json());
      if (!d.ok) throw new Error(d.error||'Failed');
    }
    document.getElementById('modal-payment-event').close();
    await loadAll();
    toast('Saved.');
  } catch(e) { showErr('pe-error', e.message); }
  finally { setBtn('pe-save', false, 'Save'); }
});

// ── Close on backdrop click ───────────────────────────────────────────────────
[
  'modal-edit-rental','modal-edit-salary','modal-edit-contract','modal-edit-invoice','modal-edit-other',
  'modal-payment-event','modal-salary','modal-rental','modal-contract','modal-invoice','modal-other',
  'modal-upload','modal-review',
].forEach(id => {
  const el = document.getElementById(id);
  if (el) el.addEventListener('click', e => { if (e.target === el) el.close(); });
});

// ── Add modals (create) ───────────────────────────────────────────────────────
function openAddModal(type) {
  document.getElementById('hero-add-menu').classList.remove('open');
  if (type === 'invoice' || type === 'contract') {
    const p = type === 'invoice' ? 'i' : 'c';
    const dlg = document.getElementById('modal-'+type);
    dlg.querySelectorAll('.modal-tab').forEach(t => t.classList.toggle('active', t.dataset.tab==='manual'));
    dlg.querySelectorAll('.tab-pane').forEach(p2 => p2.classList.toggle('active', p2.id===p+'-tab-manual'));
    document.getElementById(p+'-upload-status').textContent = '';
    showErr(p+'-upload-error','');
    document.getElementById(p+'-extract-btn').disabled = true;
    document.getElementById(p+'-file-input').value = '';
  }
  const errMap = {salary:'s-error',rental:'r-error',contract:'c-error',invoice:'i-error',other:'o-error'};
  showErr(errMap[type],'');
  if (type === 'invoice') {
    document.getElementById('i-issue').value = todayIso();
    document.getElementById('i-due').value   = '';
    document.getElementById('i-already-paid').checked = false;
    document.getElementById('i-paid-block').style.display = 'none';
    document.getElementById('i-paid-date').value = todayIso();
    // Populate contract dropdown
    const sel = document.getElementById('i-contract-link');
    sel.innerHTML = '<option value="">— None —</option>';
    _contracts.forEach(c => {
      const o = document.createElement('option');
      o.value = c.id; o.textContent = `${c.client_name} — ${c.contract_name}`;
      sel.appendChild(o);
    });
  }
  if (type === 'other') document.getElementById('o-expected').value = todayIso();
  document.getElementById('modal-'+type).showModal();
}

document.getElementById('i-already-paid').addEventListener('change', e => {
  document.getElementById('i-paid-block').style.display = e.target.checked ? '' : 'none';
  if (e.target.checked) {
    const amt = parseFloat(val('i-amount'));
    if (amt > 0) setVal('i-paid-amount', amt);
    setVal('i-paid-date', todayIso());
  }
});
document.getElementById('i-amount').addEventListener('input', e => {
  if (document.getElementById('i-already-paid').checked) {
    const v = parseFloat(e.target.value);
    if (v > 0) setVal('i-paid-amount', v);
  }
});

document.getElementById('o-recurring').addEventListener('change', e => {
  document.getElementById('o-freq-wrap').style.display = e.target.value==='1' ? '' : 'none';
});

// Save salary
document.getElementById('s-save').addEventListener('click', async () => {
  const employer      = val('s-employer').trim();
  const gross_monthly = parseFloat(val('s-gross'));
  const pay_day       = parseInt(val('s-payday'), 10);
  if (!employer)                              return showErr('s-error','Employer is required.');
  if (isNaN(gross_monthly)||gross_monthly<=0) return showErr('s-error','Enter a valid amount.');
  if (isNaN(pay_day)||pay_day<1||pay_day>31)  return showErr('s-error','Pay day must be 1–31.');
  setBtn('s-save', true, 'Saving…');
  try {
    const d = await fetch('/api/income/salaries',{
      method:'POST', headers:{'Content-Type':'application/json'},
      body: JSON.stringify({
        display_name: val('s-display-name').trim()||null,
        employer, gross_monthly, pay_day, currency:'EUR',
        role:       val('s-role').trim()||null,
        start_date: val('s-start')||null,
        end_date:   val('s-end')||null,
        notes:      val('s-notes').trim()||null,
      }),
    }).then(r=>r.json());
    if (!d.ok) throw new Error(d.error||'Save failed');
    document.getElementById('modal-salary').close();
    await loadAll(); toast('Salary added.');
  } catch(e) { showErr('s-error', e.message); }
  finally { setBtn('s-save', false, 'Save'); }
});

// Save rental
document.getElementById('r-save').addEventListener('click', async () => {
  const property_name = val('r-prop').trim();
  const monthly_rent  = parseFloat(val('r-amount'));
  const payment_day   = parseInt(val('r-paymentday'), 10);
  if (!property_name)                              return showErr('r-error','Property name is required.');
  if (isNaN(monthly_rent)||monthly_rent<=0)        return showErr('r-error','Enter a valid amount.');
  if (isNaN(payment_day)||payment_day<1||payment_day>31) return showErr('r-error','Payment day must be 1–31.');
  setBtn('r-save', true, 'Saving…');
  try {
    const d = await fetch('/api/income/rentals',{
      method:'POST', headers:{'Content-Type':'application/json'},
      body: JSON.stringify({
        display_name:   val('r-display-name').trim()||null,
        property_name, monthly_rent, payment_day, currency:'EUR',
        tenant_name:    val('r-tenant').trim()||null,
        contract_start: val('r-start')||null,
        contract_end:   val('r-end')||null,
        notes:          val('r-notes').trim()||null,
      }),
    }).then(r=>r.json());
    if (!d.ok) throw new Error(d.error||'Save failed');
    document.getElementById('modal-rental').close();
    await loadAll(); toast('Rental income added.');
  } catch(e) { showErr('r-error', e.message); }
  finally { setBtn('r-save', false, 'Save'); }
});

// Save contract
document.getElementById('c-save').addEventListener('click', async () => {
  const contract_name = val('c-name').trim();
  const client_name   = val('c-client').trim();
  if (!contract_name) return showErr('c-error','Contract name is required.');
  if (!client_name)   return showErr('c-error','Client name is required.');
  const monthly_value = parseFloat(val('c-value'));
  const total_value   = parseFloat(val('c-total'));
  setBtn('c-save', true, 'Saving…');
  try {
    const d = await fetch('/api/income/contracts',{
      method:'POST', headers:{'Content-Type':'application/json'},
      body: JSON.stringify({
        contract_name, client_name,
        contract_type:  val('c-type'),
        currency:       val('c-currency'),
        monthly_value:  isNaN(monthly_value) ? null : monthly_value,
        total_value:    isNaN(total_value)   ? null : total_value,
        start_date:     val('c-start')||null,
        end_date:       val('c-end')||null,
        project_code:   val('c-projcode').trim()||null,
        notes:          val('c-notes').trim()||null,
      }),
    }).then(r=>r.json());
    if (!d.ok) throw new Error(d.error||'Save failed');
    document.getElementById('modal-contract').close();
    await loadAll(); toast('Contract added.');
  } catch(e) { showErr('c-error', e.message); }
  finally { setBtn('c-save', false, 'Save'); }
});

// Save invoice
document.getElementById('i-save').addEventListener('click', async () => {
  const client_name    = val('i-client').trim();
  const amount         = parseFloat(val('i-amount'));
  const due_date       = val('i-due');
  if (!client_name)               return showErr('i-error','Client name is required.');
  if (isNaN(amount)||amount<=0)   return showErr('i-error','Enter a valid amount.');
  if (!due_date)                  return showErr('i-error','Due date is required.');
  const wht            = parseFloat(val('i-wht'));
  const contract_id    = parseInt(val('i-contract-link'), 10) || null;
  const alreadyPaid    = document.getElementById('i-already-paid').checked;
  const mark_paid      = alreadyPaid ? true : undefined;
  const paid_date      = alreadyPaid ? (val('i-paid-date') || todayIso()) : undefined;
  const paid_amount    = alreadyPaid ? parseFloat(val('i-paid-amount')) || amount : undefined;
  const confirmation_note = alreadyPaid ? (val('i-paid-note').trim()||null) : undefined;
  setBtn('i-save', true, 'Saving…');
  try {
    const d = await fetch('/api/income/invoices',{
      method:'POST', headers:{'Content-Type':'application/json'},
      body: JSON.stringify({
        client_name, amount, due_date,
        invoice_number:  val('i-invnum').trim()||null,
        issue_date:      val('i-issue')||null,
        withholding_tax: isNaN(wht)?null:wht,
        project_code:    val('i-projcode').trim()||null,
        contract_id,
        notes:           val('i-notes').trim()||null,
        currency:        'EUR',
        mark_paid, paid_date, paid_amount, confirmation_note,
      }),
    }).then(r=>r.json());
    if (!d.ok) throw new Error(d.error||'Save failed');
    document.getElementById('modal-invoice').close();
    await loadAll(); toast('Invoice added.');
  } catch(e) { showErr('i-error', e.message); }
  finally { setBtn('i-save', false, 'Save'); }
});

// Save other
document.getElementById('o-save').addEventListener('click', async () => {
  const description   = val('o-desc').trim();
  const amount        = parseFloat(val('o-amount'));
  if (!description)             return showErr('o-error','Description is required.');
  if (isNaN(amount)||amount<=0) return showErr('o-error','Enter a valid amount.');
  const recurring = val('o-recurring') === '1';
  setBtn('o-save', true, 'Saving…');
  try {
    const d = await fetch('/api/income/other',{
      method:'POST', headers:{'Content-Type':'application/json'},
      body: JSON.stringify({
        description, amount,
        currency:       val('o-currency'),
        expected_date:  val('o-expected')||todayIso(),
        recurring,
        cadence:        recurring ? val('o-cadence') : null,
        notes:          val('o-notes').trim()||null,
      }),
    }).then(r=>r.json());
    if (!d.ok) throw new Error(d.error||'Save failed');
    document.getElementById('modal-other').close();
    await loadAll(); toast('Income added.');
  } catch(e) { showErr('o-error', e.message); }
  finally { setBtn('o-save', false, 'Save'); }
});

// ── Modal tab switching ───────────────────────────────────────────────────────
document.addEventListener('click', e => {
  const t = e.target.closest('.modal-tab');
  if (!t || !t.dataset.modal) return;
  const modal = t.dataset.modal, tab = t.dataset.tab;
  const dlg = document.getElementById('modal-'+modal);
  if (!dlg) return;
  dlg.querySelectorAll('.modal-tab').forEach(b => b.classList.toggle('active', b.dataset.tab===tab));
  const prefix = modal==='invoice'?'i':'c';
  dlg.querySelectorAll('.tab-pane').forEach(p => p.classList.toggle('active', p.id===prefix+'-tab-'+tab));
});

// ── Invoice Upload (standalone + in-modal) ────────────────────────────────────
let _uploadFile = null, _reviewData = {};

function openInvoiceUpload() {
  openAddModal('invoice');
  const dlg = document.getElementById('modal-invoice');
  dlg.querySelectorAll('.modal-tab').forEach(t => t.classList.toggle('active', t.dataset.tab==='upload'));
  dlg.querySelectorAll('.tab-pane').forEach(p => p.classList.toggle('active', p.id==='i-tab-upload'));
}

document.getElementById('upload-file-input').addEventListener('change', e => {
  const f = e.target.files[0]; if (!f) return;
  _uploadFile = f;
  document.getElementById('upload-dropzone-label').textContent = f.name;
  document.getElementById('upload-submit').disabled = false;
});

(function(){
  const dz = document.getElementById('upload-dropzone');
  dz.addEventListener('click', () => document.getElementById('upload-file-input').click());
  dz.addEventListener('dragover', e => { e.preventDefault(); dz.classList.add('drag-over'); });
  dz.addEventListener('dragleave', () => dz.classList.remove('drag-over'));
  dz.addEventListener('drop', e => {
    e.preventDefault(); dz.classList.remove('drag-over');
    const f = e.dataTransfer.files[0]; if (!f) return;
    _uploadFile = f;
    document.getElementById('upload-dropzone-label').textContent = f.name;
    document.getElementById('upload-submit').disabled = false;
  });
})();

document.getElementById('upload-submit').addEventListener('click', async () => {
  if (!_uploadFile) return;
  setBtn('upload-submit', true, 'Extracting…');
  document.getElementById('upload-status').textContent = 'Uploading and extracting data…';
  showErr('upload-error','');
  try {
    const fd = new FormData(); fd.append('file', _uploadFile, _uploadFile.name);
    const d = await fetch('/api/income/invoices/upload',{method:'POST',body:fd}).then(r=>r.json());
    if (!d.ok) throw new Error(d.error||'Extraction failed');
    document.getElementById('modal-upload').close();
    await openReviewModal(d);
  } catch(e) { showErr('upload-error', e.message); document.getElementById('upload-status').textContent=''; }
  finally { setBtn('upload-submit', false, 'Extract'); }
});

// ── In-modal invoice upload ───────────────────────────────────────────────────
let _iFile = null;

document.getElementById('i-file-input').addEventListener('change', e => {
  const f = e.target.files[0]; if (!f) return;
  _iFile = f;
  document.getElementById('i-dropzone-label').textContent = f.name;
  document.getElementById('i-extract-btn').disabled = false;
});

(function(){
  const dz = document.getElementById('i-dropzone');
  dz.addEventListener('click', e => { if (e.target.tagName!=='U') document.getElementById('i-file-input').click(); });
  dz.addEventListener('dragover', e => { e.preventDefault(); dz.classList.add('drag-over'); });
  dz.addEventListener('dragleave', () => dz.classList.remove('drag-over'));
  dz.addEventListener('drop', e => {
    e.preventDefault(); dz.classList.remove('drag-over');
    const f = e.dataTransfer.files[0]; if (!f) return;
    _iFile = f;
    document.getElementById('i-dropzone-label').textContent = f.name;
    document.getElementById('i-extract-btn').disabled = false;
  });
})();

document.getElementById('i-extract-btn').addEventListener('click', async () => {
  if (!_iFile) return;
  setBtn('i-extract-btn', true, 'Extracting…');
  document.getElementById('i-upload-status').textContent = 'Uploading and extracting data…';
  showErr('i-upload-error','');
  try {
    const fd = new FormData(); fd.append('file', _iFile, _iFile.name);
    const d = await fetch('/api/income/invoices/upload',{method:'POST',body:fd}).then(r=>r.json());
    if (!d.ok) throw new Error(d.error||'Extraction failed');
    document.getElementById('modal-invoice').close();
    await openReviewModal(d);
  } catch(e) { showErr('i-upload-error', e.message); document.getElementById('i-upload-status').textContent=''; }
  finally { setBtn('i-extract-btn', false, 'Extract'); }
});

// ── In-modal contract upload ──────────────────────────────────────────────────
let _cFile = null;

document.getElementById('c-file-input').addEventListener('change', e => {
  const f = e.target.files[0]; if (!f) return;
  _cFile = f;
  document.getElementById('c-dropzone-label').textContent = f.name;
  document.getElementById('c-extract-btn').disabled = false;
});

(function(){
  const dz = document.getElementById('c-dropzone');
  dz.addEventListener('click', e => { if (e.target.tagName!=='U') document.getElementById('c-file-input').click(); });
  dz.addEventListener('dragover', e => { e.preventDefault(); dz.classList.add('drag-over'); });
  dz.addEventListener('dragleave', () => dz.classList.remove('drag-over'));
  dz.addEventListener('drop', e => {
    e.preventDefault(); dz.classList.remove('drag-over');
    const f = e.dataTransfer.files[0]; if (!f) return;
    _cFile = f;
    document.getElementById('c-dropzone-label').textContent = f.name;
    document.getElementById('c-extract-btn').disabled = false;
  });
})();

document.getElementById('c-extract-btn').addEventListener('click', async () => {
  if (!_cFile) return;
  setBtn('c-extract-btn', true, 'Extracting…');
  document.getElementById('c-upload-status').textContent = 'Uploading and extracting data…';
  showErr('c-upload-error','');
  try {
    const fd = new FormData(); fd.append('file', _cFile, _cFile.name);
    const d = await fetch('/api/income/contracts/upload',{method:'POST',body:fd}).then(r=>r.json());
    if (!d.ok) throw new Error(d.error||'Extraction failed');
    const ex = d.extracted||{};
    setVal('c-name',    ex.contract_name||''); setVal('c-client', ex.client_name||'');
    setVal('c-type',    ex.contract_type||'fixed'); setVal('c-value', ex.monthly_value??'');
    setVal('c-total',   ex.total_value??''); setVal('c-start', ex.start_date||'');
    setVal('c-end',     ex.end_date||''); setVal('c-projcode', ex.project_code||'');
    setVal('c-notes',   ex.notes||'');
    const dlg = document.getElementById('modal-contract');
    dlg.querySelectorAll('.modal-tab').forEach(t => t.classList.toggle('active', t.dataset.tab==='manual'));
    dlg.querySelectorAll('.tab-pane').forEach(p => p.classList.toggle('active', p.id==='c-tab-manual'));
    showErr('c-error',''); toast('Extracted — review and save.');
  } catch(e) { showErr('c-upload-error', e.message); document.getElementById('c-upload-status').textContent=''; }
  finally { setBtn('c-extract-btn', false, 'Extract'); }
});

// ── Review extracted invoice ──────────────────────────────────────────────────
async function openReviewModal(uploadResult) {
  const ex = uploadResult.extracted||{};
  _reviewData = uploadResult;
  const sel = document.getElementById('rv-contract');
  sel.innerHTML = '<option value="">— None —</option>';
  (_contracts||[]).forEach(c => {
    const o = document.createElement('option');
    o.value = c.id; o.textContent = `${c.client_name} — ${c.contract_name}`;
    sel.appendChild(o);
  });
  if (uploadResult.matched_contract_id) sel.value = String(uploadResult.matched_contract_id);
  setVal('rv-client',  ex.client_name||'');
  setVal('rv-tax-id',  ex.client_tax_id||'');
  setVal('rv-invnum',  ex.invoice_number||'');
  setVal('rv-projcode',ex.project_code||'');
  setVal('rv-issue',   ex.issue_date||'');
  setVal('rv-due',     ex.due_date||'');
  setVal('rv-gross',   ex.amount_gross??'');
  setVal('rv-net',     ex.amount_net??'');
  setVal('rv-wht',     ex.withholding_tax??'');
  setVal('rv-currency',ex.currency||'EUR');
  setVal('rv-status',  'draft');
  setVal('rv-notes',   [ex.description,ex.notes].filter(Boolean).join(' | '));
  const matchedEl = document.getElementById('review-matched-contract');
  if (uploadResult.matched_contract_id) {
    const c = (_contracts||[]).find(c => c.id==uploadResult.matched_contract_id);
    matchedEl.textContent = c?`Matched contract: ${c.client_name} — ${c.contract_name}`:`Matched contract id=${uploadResult.matched_contract_id}`;
    matchedEl.style.display='';
  } else { matchedEl.style.display='none'; }
  showErr('rv-error','');
  document.getElementById('modal-review').showModal();
}

document.getElementById('rv-save').addEventListener('click', async () => {
  const client_name = val('rv-client').trim();
  const amount      = parseFloat(val('rv-gross'));
  const issue_date  = val('rv-issue');
  if (!client_name)                return showErr('rv-error','Client name is required.');
  if (isNaN(amount)||amount<=0)    return showErr('rv-error','Enter a valid gross amount.');
  if (!issue_date)                 return showErr('rv-error','Issue date is required.');
  const amount_net        = parseFloat(val('rv-net'))||null;
  const withholding_tax   = parseFloat(val('rv-wht'))||null;
  const contract_id       = parseInt(val('rv-contract'))||null;
  const attached_file_path= (_reviewData.extracted||{}).attached_file_path||null;
  setBtn('rv-save', true, 'Saving…');
  try {
    const d = await fetch('/api/income/invoices',{
      method:'POST', headers:{'Content-Type':'application/json'},
      body: JSON.stringify({
        client_name, amount, issue_date,
        due_date:        val('rv-due')||issue_date,
        invoice_number:  val('rv-invnum').trim()||null,
        project_code:    val('rv-projcode').trim()||null,
        contract_id, currency: val('rv-currency'),
        status:          val('rv-status'),
        notes:           val('rv-notes').trim()||null,
        client_tax_id:   val('rv-tax-id').trim()||null,
        amount_net, withholding_tax, attached_file_path,
      }),
    }).then(r=>r.json());
    if (!d.ok) throw new Error(d.error||'Save failed');
    document.getElementById('modal-review').close();
    await loadAll(); toast('Invoice saved.');
  } catch(e) { showErr('rv-error', e.message); }
  finally { setBtn('rv-save', false, 'Create Invoice'); }
});

// ── Boot ──────────────────────────────────────────────────────────────────────
window.addEventListener('DOMContentLoaded', async () => {
  requestAnimationFrame(() => document.body.classList.add('loaded'));
  updateYearLabel();
  switchView(_currentView);
  updateMonthLabel();
  await loadAll();
});

// ════════════════════════════════════════════════════════════════════════════
// PASS B — YEARLY VIEW + INVOICE PAYMENTS
// ════════════════════════════════════════════════════════════════════════════

// ── Yearly state ──────────────────────────────────────────────────────────────
let _currentYear      = parseInt(localStorage.getItem('income.year')) || new Date().getFullYear();
let _yearChartData    = {};   // cache: { year: [{month, gross, by_source}] }
let _eiPaymentsChanged = false;

function _todayYear()  { return new Date().getFullYear(); }
function _todayMonth() { return new Date().getMonth(); }  // 0-based

function setYear(y) {
  _currentYear = y;
  localStorage.setItem('income.year', String(y));
  delete _yearChartData[y];
  updateYearLabel();
  loadYearly();
}

function updateYearLabel() {
  const lbl = _currentYear === _todayYear()
    ? `YEAR · ${_currentYear}`
    : `FULL YEAR · ${_currentYear}`;
  const el = document.getElementById('year-label');
  if (el) el.textContent = lbl;
}

document.getElementById('btn-prev-year').addEventListener('click', () => setYear(_currentYear - 1));
document.getElementById('btn-next-year').addEventListener('click', () => setYear(_currentYear + 1));

// Year picker dropdown
let _yrPickerOpen = false;
document.getElementById('year-label').addEventListener('click', e => {
  e.stopPropagation();
  const dd = document.getElementById('year-picker-dd');
  _yrPickerOpen = !_yrPickerOpen;
  if (_yrPickerOpen) {
    dd.innerHTML = '';
    for (let y = _todayYear() + 1; y >= 2020; y--) {
      const b = document.createElement('button');
      b.className = 'year-picker-opt' + (y === _currentYear ? ' selected' : '');
      b.textContent = String(y);
      b.addEventListener('click', ev => {
        ev.stopPropagation();
        _yrPickerOpen = false;
        dd.style.display = 'none';
        setYear(y);
      });
      dd.appendChild(b);
    }
    dd.style.display = '';
  } else {
    dd.style.display = 'none';
  }
});
document.addEventListener('click', () => {
  if (_yrPickerOpen) {
    _yrPickerOpen = false;
    const dd = document.getElementById('year-picker-dd');
    if (dd) dd.style.display = 'none';
  }
});

// ── Load yearly (all phases) ──────────────────────────────────────────────────
async function loadYearly() {
  updateYearLabel();
  await Promise.all([loadYearlyHero(), loadYearChart()]);
  loadYearSourceSummary();
}

// ── Yearly hero ────────────────────────────────────────────────────────────────
async function loadYearlyHero() {
  try {
    const d = await fetch(
      `/api/income/summary?year=${_currentYear}&month=${_currentYear}-01`
    ).then(r => r.json());
    if (!d.ok) throw new Error(d.error || 'Failed');
    const ytd = d.summary.year_to_date || {};
    renderYearlyHero(ytd);
  } catch (e) {
    const el = document.getElementById('yr-gross');
    if (el) el.textContent = '—';
  }
}

function renderYearlyHero(ytd) {
  const gross = ytd.gross || 0, net = ytd.net || 0;
  const withholding = ytd.withholding || 0, pending = ytd.pending_gross || 0;
  const by = ytd.by_source || {};

  document.getElementById('yr-gross').textContent = gross > 0.005 ? fmtEur(gross) : '—';
  document.getElementById('yr-net').textContent   = gross > 0.005 ? fmtEur(net) + ' net' : '—';

  const withEl = document.getElementById('yr-withholding');
  if (withholding > 0.005) {
    withEl.textContent = 'after ' + fmtEur(withholding) + ' withholding';
    withEl.style.display = '';
  } else { withEl.style.display = 'none'; }

  const pendEl = document.getElementById('yr-pending');
  if (pending > 0.005) {
    pendEl.textContent = '+' + fmtEur(pending) + ' expected';
    pendEl.style.display = '';
  } else { pendEl.style.display = 'none'; }

  const chips = [
    { label: 'Invoices', key: 'invoices', color: '#fbbf24' },
    { label: 'Rentals',  key: 'rentals',  color: '#5eead4' },
    { label: 'Salaries', key: 'salaries', color: '#c4b5fd' },
    { label: 'Other',    key: 'other',    color: '#94a3b8' },
  ].map(({ label, key, color }) => {
    const g = (by[key] || {}).gross || 0;
    if (g < 0.005) return '';
    return `<div class="hero-chip"><div class="hero-chip-dot" style="background:${color}"></div>${esc(label)} ${fmtEur(g)}</div>`;
  }).filter(Boolean).join('');

  document.getElementById('yr-breakdown').innerHTML =
    chips || '<div class="hero-chip" style="color:rgba(255,255,255,.22)">No income this year</div>';
}

// ── Bar chart ──────────────────────────────────────────────────────────────────
const _MONTHS_ABBR = ['JAN','FEB','MAR','APR','MAY','JUN','JUL','AUG','SEP','OCT','NOV','DEC'];

function fmtCompact(n) {
  if (n < 0.005) return '—';
  if (n >= 1000) {
    const k = n / 1000;
    return '€' + (k % 1 === 0 ? k.toFixed(0) : k.toFixed(1)) + 'K';
  }
  return fmtEur(n);
}

async function loadYearChart() {
  const year = _currentYear;
  if (_yearChartData[year]) { renderBarChart(_yearChartData[year], year); return; }

  const results = await Promise.all(
    Array.from({ length: 12 }, (_, i) => {
      const m = `${year}-${String(i + 1).padStart(2, '0')}`;
      return fetch(`/api/income/summary?year=${year}&month=${m}`)
        .then(r => r.json()).catch(() => null);
    })
  );

  const data = results.map((d, i) => {
    if (!d || !d.ok) return { month: i, gross: 0, by_source: {} };
    const mon = d.summary.month || {};
    return { month: i, gross: mon.gross || 0, by_source: mon.by_source || {} };
  });
  _yearChartData[year] = data;
  renderBarChart(data, year);
}

function renderBarChart(data, year) {
  const el = document.getElementById('yr-chart');
  if (!el) return;
  el.innerHTML = '';
  const tooltip = document.getElementById('yr-tooltip');
  const CHART_H = 140;  // max bar height in px
  const maxVal  = Math.max(...data.map(d => d.gross), 1);
  const nowY = _todayYear(), nowM = _todayMonth();

  data.forEach((d, i) => {
    const isFuture = year > nowY || (year === nowY && i > nowM);
    const col = document.createElement('div');
    col.className = 'yr-bar-col' + (isFuture ? ' yr-bar-future' : '');

    // Spacer pushes bar to bottom of the col
    const spacer = document.createElement('div');
    spacer.style.flex = '1';

    let barEl;
    if (d.gross < 0.005) {
      barEl = document.createElement('div');
      barEl.className = 'yr-bar-hairline' + (isFuture ? ' future' : '');
    } else {
      const totalH = Math.max(Math.round((d.gross / maxVal) * CHART_H), 4);
      barEl = document.createElement('div');
      barEl.className = 'yr-bar-stack';
      barEl.style.height = totalH + 'px';
      barEl.style.animation = `yr-bar-grow 0.35s ease-out ${i * 45}ms both`;

      // Segments bottom→top: rental, invoice, salary, other
      // column-reverse: first DOM child = visual bottom
      const src = d.by_source || {};
      [
        { gross: (src.rentals  || {}).gross || 0, color: 'rgba(94,234,212,.7)' },
        { gross: (src.invoices || {}).gross || 0, color: 'rgba(251,191,36,.7)' },
        { gross: (src.salaries || {}).gross || 0, color: 'rgba(196,181,253,.7)' },
        { gross: (src.other    || {}).gross || 0, color: 'rgba(148,163,184,.7)' },
      ].forEach(seg => {
        if (seg.gross < 0.005) return;
        const s = document.createElement('div');
        s.style.flex = String(seg.gross);
        s.style.background = seg.color;
        s.style.width = '100%';
        barEl.appendChild(s);
      });
    }

    const mlbl = document.createElement('div');
    mlbl.className = 'yr-bar-month';
    mlbl.textContent = _MONTHS_ABBR[i];

    const albl = document.createElement('div');
    albl.className = 'yr-bar-amount';
    albl.textContent = fmtCompact(d.gross);

    col.append(spacer, barEl, mlbl, albl);
    el.appendChild(col);

    // Tooltip
    col.addEventListener('mouseenter', e => {
      const src2 = d.by_source || {};
      tooltip.innerHTML = `
        <div class="yr-tooltip-title">${_MONTHS_ABBR[i]} ${year}</div>
        ${[['Rental',(src2.rentals||{}).gross||0],['Invoice',(src2.invoices||{}).gross||0],
           ['Salary',(src2.salaries||{}).gross||0],['Other',(src2.other||{}).gross||0]]
          .map(([l,v]) => `<div class="yr-tooltip-row"><span>${l}</span><span>${fmtEur(v)}</span></div>`).join('')}
        <div class="yr-tooltip-divider"></div>
        <div class="yr-tooltip-total"><span>Total</span><span>${fmtEur(d.gross)}</span></div>`;
      tooltip.classList.add('visible');
      _posTooltip(e);
    });
    col.addEventListener('mousemove', _posTooltip);
    col.addEventListener('mouseleave', () => tooltip.classList.remove('visible'));

    // Click → switch to monthly
    col.addEventListener('click', () => {
      setMonth(`${year}-${String(i + 1).padStart(2, '0')}`);
      switchView('monthly');
    });
  });
}

function _posTooltip(e) {
  const tt = document.getElementById('yr-tooltip');
  if (!tt) return;
  const tw = tt.offsetWidth || 200, th = tt.offsetHeight || 130;
  let x = e.clientX + 14, y = e.clientY - th - 10;
  if (x + tw > window.innerWidth - 8) x = e.clientX - tw - 14;
  if (y < 8) y = e.clientY + 14;
  tt.style.left = x + 'px';
  tt.style.top  = y + 'px';
}

// ── Source summary ─────────────────────────────────────────────────────────────
async function loadYearSourceSummary() {
  const el = document.getElementById('yr-sources');
  if (!el) return;
  try {
    const d = await fetch(
      `/api/income/payment-events?start=${_currentYear}-01-01&end=${_currentYear}-12-31`
    ).then(r => r.json());
    const events = d.events || d.payment_events || [];

    const groups = {};
    events.forEach(ev => {
      const key = `${ev.source_type}:${ev.source_id}`;
      if (!groups[key]) groups[key] = {
        source_type: ev.source_type, source_id: ev.source_id,
        display_name: ev.display_name || '—', received: 0, expected: 0,
      };
      if (ev.status === 'received') groups[key].received += Number(ev.amount) || 0;
      else                          groups[key].expected += Number(ev.amount) || 0;
    });

    const rows = Object.values(groups);
    if (!rows.length) {
      el.innerHTML = '<div class="income-empty" style="padding:12px 16px">No activity this year.</div>';
      return;
    }

    el.innerHTML = rows.map(row => {
      const typeLabel = { invoice:'Invoice', rental:'Rental', salary:'Salary', other:'Other' }[row.source_type] || row.source_type;
      let statusHtml = '';
      if (row.source_type === 'invoice') {
        const inv = _invoices.find(i => i.id == row.source_id);
        statusHtml = inv ? pillHtml(invoicePillInfo(inv).cls, invoicePillInfo(inv).text, false) : '';
      } else {
        const tot = row.received + row.expected;
        const pct = tot > 0.005 ? Math.round(row.received / tot * 100) : 0;
        statusHtml = pillHtml(pct >= 100 ? 's-paid' : 's-expected', pct + '%', false);
      }
      const expCol = row.source_type !== 'invoice' && row.expected > 0.005 ? fmtEur(row.expected) : '—';
      return `<div class="yr-source-row" data-src-type="${esc(row.source_type)}" data-src-id="${row.source_id}">
        <div class="yr-source-name">${esc(row.display_name)}</div>
        <div class="yr-source-type">${esc(typeLabel)}</div>
        <div class="yr-source-received">${fmtEur(row.received)}</div>
        <div class="yr-source-expected">${expCol}</div>
        <div class="yr-source-status">${statusHtml}</div>
      </div>`;
    }).join('');

    el.querySelectorAll('.yr-source-row').forEach(row => {
      row.addEventListener('click', () => {
        const t = row.dataset.srcType, id = parseInt(row.dataset.srcId, 10);
        if (t === 'invoice') {
          const inv = _invoices.find(i => i.id === id);
          if (inv) openEditModal('invoice', inv);
        } else {
          _openEditById(t, id);
        }
      });
    });
  } catch (e) {
    el.innerHTML = `<div class="income-empty" style="color:#c47a7a;padding:12px 16px">${esc(e.message)}</div>`;
  }
}

async function _openEditById(type, id) {
  try {
    const d = await fetch(`/api/income/${URL_MAP[type]}`).then(r => r.json());
    const list = d[URL_MAP[type]] || [];
    const rec = list.find(r => r.id === id);
    if (rec) openEditModal(type, rec); else toast('Record not found.');
  } catch (e) { toast('Error: ' + e.message); }
}

// ── Invoice payments subsection ───────────────────────────────────────────────
async function loadInvoicePayments(invoice) {
  const el = document.getElementById('ei-payments-section');
  if (!el) return;
  el.innerHTML = '';
  try {
    const d = await fetch(
      `/api/income/payment-events?source_type=invoice&source_id=${invoice.id}`
    ).then(r => r.json());
    renderInvoicePayments(invoice, d.events || d.payment_events || []);
  } catch (e) {
    el.innerHTML = `<div style="font-size:12px;color:#c47a7a;padding:4px 0">${esc(e.message)}</div>`;
  }
}

function renderInvoicePayments(invoice, events) {
  const el = document.getElementById('ei-payments-section');
  if (!el) return;
  const total  = events.reduce((s, e) => s + (Number(e.amount) || 0), 0);
  const amount = Number(invoice.amount) || 0;

  let summaryHtml = '';
  if (total > 0.005) {
    if (total > amount && amount > 0) {
      summaryHtml = `<div class="payments-summary s-over">Total received: ${fmtEur(total)} of ${fmtEur(amount)} · oversubscribed</div>`;
    } else if (total >= amount && amount > 0) {
      summaryHtml = `<div class="payments-summary s-paid">Total received: ${fmtEur(total)} · paid in full</div>`;
    } else {
      summaryHtml = `<div class="payments-summary s-partial">Total received: ${fmtEur(total)} of ${fmtEur(amount)} · ${fmtEur(amount - total)} remaining</div>`;
    }
  }

  el.innerHTML = `
    <div class="hero-hairline" style="margin:14px 0 12px"></div>
    <div class="payments-section-hdr">PAYMENTS</div>
    <div id="ei-pay-rows">
      ${events.length ? events.map(ev => `
        <div class="payment-row" data-ev-id="${ev.id}">
          <div class="payment-amount">${fmtEur(ev.amount)}</div>
          <div class="payment-meta">${fmtDate(ev.paid_date)}${ev.confirmation_note ? ' · ' + esc(ev.confirmation_note) : ''}</div>
          <div class="payment-actions">
            <button class="btn-pay-action" data-act="edit" data-ev-id="${ev.id}">Edit</button>
            <button class="btn-pay-action btn-pay-del" data-act="del" data-ev-id="${ev.id}">Delete</button>
          </div>
        </div>`).join('') :
        '<div style="font-size:12px;color:rgba(255,255,255,.25);padding:4px 0">No payments recorded</div>'
      }
    </div>
    ${summaryHtml}
    <div id="ei-pay-add-area">
      <button class="btn-add-payment" id="btn-ei-add-pay">+ Add payment</button>
    </div>`;

  el.querySelectorAll('[data-act="edit"]').forEach(btn => {
    btn.addEventListener('click', ev => {
      ev.stopPropagation();
      const evId = parseInt(btn.dataset.evId, 10);
      const evObj = events.find(e => e.id === evId);
      if (evObj) _showPayEditForm(evObj, invoice);
    });
  });
  el.querySelectorAll('[data-act="del"]').forEach(btn => {
    btn.addEventListener('click', ev => {
      ev.stopPropagation();
      _showPayDelConfirm(parseInt(btn.dataset.evId, 10), invoice);
    });
  });
  document.getElementById('btn-ei-add-pay').addEventListener('click', () => _showPayAddForm(invoice, events));
}

function _showPayEditForm(ev, invoice) {
  const row = document.querySelector(`#ei-pay-rows .payment-row[data-ev-id="${ev.id}"]`);
  if (!row) return;
  const form = document.createElement('div');
  form.className = 'payment-inline-form';
  form.innerHTML = `
    <div class="modal-row">
      <div class="modal-field"><label class="modal-label">Date</label>
        <input class="modal-input" type="date" id="pef-date" value="${esc(ev.paid_date || '')}"/></div>
      <div class="modal-field"><label class="modal-label">Amount (€)</label>
        <input class="modal-input" type="number" id="pef-amount" step="0.01" value="${Number(ev.amount) || ''}"/></div>
    </div>
    <div class="modal-field"><label class="modal-label">Note</label>
      <input class="modal-input" type="text" id="pef-note" value="${esc(ev.confirmation_note || '')}"/></div>
    <div id="pef-err" class="modal-error"></div>
    <div class="payment-inline-actions">
      <button class="btn-modal-cancel" id="pef-cancel" style="padding:5px 10px;font-size:12px">Cancel</button>
      <button class="btn-modal-save" id="pef-save" style="padding:5px 12px;font-size:12px">Save</button>
    </div>`;
  row.replaceWith(form);
  document.getElementById('pef-cancel').addEventListener('click', () => loadInvoicePayments(invoice));
  document.getElementById('pef-save').addEventListener('click', async () => {
    const date   = document.getElementById('pef-date').value;
    const amount = parseFloat(document.getElementById('pef-amount').value);
    const note   = document.getElementById('pef-note').value.trim() || null;
    if (!date || isNaN(amount) || amount <= 0) return showErr('pef-err', 'Valid date and amount required.');
    const btn = document.getElementById('pef-save');
    btn.disabled = true; btn.textContent = 'Saving…';
    try {
      const d = await fetch(`/api/income/payment-events/${ev.id}`, {
        method: 'PATCH', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ paid_date: date, amount, confirmation_note: note }),
      }).then(r => r.json());
      if (!d.ok) throw new Error(d.error || 'Failed');
      _eiPaymentsChanged = true;
      await loadInvoicePayments(invoice);
    } catch (err) { showErr('pef-err', err.message); btn.disabled = false; btn.textContent = 'Save'; }
  });
}

function _showPayDelConfirm(evId, invoice) {
  const row = document.querySelector(`#ei-pay-rows .payment-row[data-ev-id="${evId}"]`);
  if (!row) return;
  const c = document.createElement('div');
  c.className = 'payment-inline-form';
  c.style.cssText = 'display:flex;align-items:center;gap:10px';
  c.innerHTML = `
    <span style="flex:1;font-size:12px;color:rgba(255,255,255,.5)">Delete this payment?</span>
    <button class="btn-modal-cancel" id="pdc-cancel" style="padding:5px 10px;font-size:12px">Cancel</button>
    <button class="btn-modal-danger-confirm" id="pdc-yes">Yes</button>`;
  row.replaceWith(c);
  document.getElementById('pdc-cancel').addEventListener('click', () => loadInvoicePayments(invoice));
  document.getElementById('pdc-yes').addEventListener('click', async () => {
    const btn = document.getElementById('pdc-yes');
    btn.disabled = true; btn.textContent = 'Deleting…';
    try {
      const d = await fetch(`/api/income/payment-events/${evId}`, { method: 'DELETE' }).then(r => r.json());
      if (!d.ok) throw new Error(d.error || 'Failed');
      _eiPaymentsChanged = true;
      await loadInvoicePayments(invoice);
    } catch (e) { btn.disabled = false; btn.textContent = 'Yes'; toast('Error: ' + e.message); }
  });
}

function _showPayAddForm(invoice, events) {
  const area = document.getElementById('ei-pay-add-area');
  if (!area) return;
  const received   = events.reduce((s, e) => s + (Number(e.amount) || 0), 0);
  const invAmt     = Number(invoice.amount) || 0;
  const defaultAmt = Math.max(invAmt - received, 0) || invAmt;
  area.innerHTML = `
    <div class="payment-inline-form">
      <div class="modal-row">
        <div class="modal-field"><label class="modal-label">Date</label>
          <input class="modal-input" type="date" id="paf-date" value="${todayIso()}"/></div>
        <div class="modal-field"><label class="modal-label">Amount (€)</label>
          <input class="modal-input" type="number" id="paf-amount" step="0.01" value="${defaultAmt > 0 ? defaultAmt : ''}"/></div>
      </div>
      <div class="modal-field"><label class="modal-label">Note (optional)</label>
        <input class="modal-input" type="text" id="paf-note" placeholder="e.g. bank transfer"/></div>
      <div id="paf-err" class="modal-error"></div>
      <div class="payment-inline-actions">
        <button class="btn-modal-cancel" id="paf-cancel" style="padding:5px 10px;font-size:12px">Cancel</button>
        <button class="btn-modal-save" id="paf-add" style="padding:5px 12px;font-size:12px">Add</button>
      </div>
    </div>`;
  document.getElementById('paf-cancel').addEventListener('click', () => loadInvoicePayments(invoice));
  document.getElementById('paf-add').addEventListener('click', async () => {
    const date   = document.getElementById('paf-date').value;
    const amount = parseFloat(document.getElementById('paf-amount').value);
    const note   = document.getElementById('paf-note').value.trim() || null;
    if (!date || isNaN(amount) || amount <= 0) return showErr('paf-err', 'Valid date and amount required.');
    const btn = document.getElementById('paf-add');
    btn.disabled = true; btn.textContent = 'Adding…';
    try {
      const d = await fetch(`/api/income/invoices/${invoice.id}/payments`, {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ paid_date: date, amount, note }),
      }).then(r => r.json());
      if (!d.ok) throw new Error(d.error || 'Failed');
      _eiPaymentsChanged = true;
      await loadInvoicePayments(invoice);
    } catch (e) { showErr('paf-err', e.message); btn.disabled = false; btn.textContent = 'Add'; }
  });
}

// Refresh page data when invoice modal closes (if payments were mutated)
document.getElementById('modal-edit-invoice').addEventListener('close', () => {
  if (_eiPaymentsChanged) {
    _eiPaymentsChanged = false;
    loadAll();
  }
});
