/**
 * Sterngate Main Application Logic
 */

let lastTelemetrySnap = null;

function updateCylBar(idVal, idBar, val) {
  if (val !== null && val !== undefined) {
    document.getElementById(idVal).textContent = (val >= 0 ? '+' : '') + val.toFixed(2) + ' mm³';
    const pct = Math.min(Math.max((val + 3.0) / 6.0 * 100, 5), 95);
    const bar = document.getElementById(idBar);
    bar.style.width = pct + '%';
    bar.style.background = Math.abs(val) > 2.0 ? 'var(--warning)' : 'var(--success)';
  }
}

function updateTransStatus(temp) {
  if (temp === null || temp === undefined) return;
  const badge = document.getElementById('badge-trans-status');
  if (!badge) return;

  if (temp < 78.0) {
    badge.className = 'badge';
    badge.style.background = 'rgba(88, 166, 255, 0.2)';
    badge.style.color = 'var(--accent)';
    badge.style.border = '1px solid var(--accent)';
    badge.textContent = i18n.t('telemetry.atf_warming').replace('{temp}', temp);
  } else if (temp <= 82.0) {
    badge.className = 'badge badge-ready pulse';
    badge.style.background = 'rgba(46, 160, 67, 0.25)';
    badge.style.color = '#3fb950';
    badge.style.border = '1px solid #3fb950';
    badge.textContent = i18n.t('telemetry.atf_ready').replace('{temp}', temp);
  } else {
    badge.className = 'badge';
    badge.style.background = 'rgba(210, 153, 34, 0.2)';
    badge.style.color = 'var(--warning)';
    badge.style.border = '1px solid var(--warning)';
    badge.textContent = i18n.t('telemetry.atf_overheat').replace('{temp}', temp);
  }
}

// Handle language change for live dynamic strings
window.addEventListener('languageChanged', () => {
  if (lastTelemetrySnap && lastTelemetrySnap.trans_fluid_temp !== null) {
    updateTransStatus(lastTelemetrySnap.trans_fluid_temp);
  }
});

// Setup telemetry WebSocket
function setupTelemetryWebSocket() {
  const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
  const ws = new WebSocket(`${protocol}//${location.host}/ws/telemetry`);

  ws.onmessage = (e) => {
    try {
      const snap = JSON.parse(e.data);
      lastTelemetrySnap = snap;

      if (snap.engine_rpm !== null && snap.engine_rpm !== undefined) {
        document.getElementById('val-rpm').textContent = Math.round(snap.engine_rpm);
      }
      if (snap.coolant_temp !== null && snap.coolant_temp !== undefined) {
        document.getElementById('val-coolant').textContent = snap.coolant_temp + '°C';
      }
      if (snap.trans_fluid_temp !== null && snap.trans_fluid_temp !== undefined) {
        document.getElementById('val-trans-temp').textContent = snap.trans_fluid_temp + '°C';
        updateTransStatus(snap.trans_fluid_temp);
      }
      if (snap.rail_pressure !== null && snap.rail_pressure !== undefined) {
        document.getElementById('val-rail').textContent = snap.rail_pressure.toFixed(1) + ' bar';
      }
      if (snap.boost_pressure !== null && snap.boost_pressure !== undefined) {
        document.getElementById('val-boost').textContent = Math.round(snap.boost_pressure) + ' hPa';
      }
      if (snap.tcc_slip_rpm !== null && snap.tcc_slip_rpm !== undefined) {
        document.getElementById('val-tcc').textContent = Math.round(snap.tcc_slip_rpm) + ' RPM';
      }
      if (snap.battery_voltage !== null && snap.battery_voltage !== undefined) {
        document.getElementById('badge-voltage').textContent = snap.battery_voltage.toFixed(1) + 'V';
      }

      updateCylBar('val-cyl1', 'bar-cyl1', snap.inj_corr_cyl1);
      updateCylBar('val-cyl2', 'bar-cyl2', snap.inj_corr_cyl2);
      updateCylBar('val-cyl3', 'bar-cyl3', snap.inj_corr_cyl3);
      updateCylBar('val-cyl4', 'bar-cyl4', snap.inj_corr_cyl4);
    } catch(err) {}
  };

  ws.onclose = () => {
    setTimeout(setupTelemetryWebSocket, 2000);
  };
}

// Fallback telemetry fetch
async function fetchTelemetry() {
  try {
    const res = await fetch('/api/v1/telemetry');
    const snap = await res.json();
    if (snap.engine_rpm) document.getElementById('val-rpm').textContent = Math.round(snap.engine_rpm);
  } catch(e) {}
}

// DTC Scanner
async function scanDtc() {
  const lang = i18n.currentLang || 'en';
  const res = await fetch(`/api/v1/dtc?lang=${lang}`);
  const data = await res.json();
  const tbody = document.getElementById('dtc-table');
  if (data.length === 0) {
    tbody.innerHTML = `<tr><td colspan="4" style="color: var(--success); text-align: center;">${i18n.t('dtc.no_codes')}</td></tr>`;
  } else {
    tbody.innerHTML = data.map(d => `
      <tr>
        <td><b>${d.code}</b></td>
        <td>${d.module}</td>
        <td>${d.description}</td>
        <td><span style="color: var(--warning)">${d.confirmed ? i18n.t('dtc.confirmed') : i18n.t('dtc.active')}</span></td>
      </tr>
    `).join('');
  }
}

async function clearDtc() {
  await fetch('/api/v1/dtc/clear', { method: 'POST' });
  document.getElementById('dtc-table').innerHTML = `<tr><td colspan="4" style="color: var(--success); text-align: center;">${i18n.t('dtc.cleared')}</td></tr>`;
}

// Flight Telemetry Recorder
let recTimer = null;
let recElapsedSec = 0;

async function pollRecorderStatus() {
  try {
    const res = await fetch('/api/v1/recorder/status');
    const status = await res.json();
    const badge = document.getElementById('rec-status-badge');
    const btn = document.getElementById('btn-rec-toggle');
    const fileSpan = document.getElementById('rec-file-path');
    const rowsSpan = document.getElementById('rec-rows-count');

    if (status.is_recording) {
      badge.textContent = i18n.t('recorder.status_recording');
      badge.className = 'badge badge-recording pulse';
      btn.textContent = i18n.t('recorder.btn_stop');
      btn.className = 'btn btn-danger';
      fileSpan.textContent = status.current_file || 'active_run.csv';
      rowsSpan.textContent = status.records_count;
      recElapsedSec = status.elapsed_seconds;
      document.getElementById('rec-elapsed').textContent = recElapsedSec + 's';
      if (!recTimer) {
        recTimer = setInterval(() => {
          recElapsedSec++;
          document.getElementById('rec-elapsed').textContent = recElapsedSec + 's';
        }, 1000);
      }
    } else {
      badge.textContent = i18n.t('recorder.status_idle');
      badge.className = 'badge';
      badge.style.background = '#21262d';
      badge.style.color = 'var(--text-muted)';
      badge.style.border = '1px solid var(--border)';
      btn.textContent = i18n.t('recorder.btn_start');
      btn.className = 'btn btn-primary';
      if (status.current_file) fileSpan.textContent = status.current_file;
      rowsSpan.textContent = status.records_count;
      if (recTimer) {
        clearInterval(recTimer);
        recTimer = null;
      }
    }
  } catch(e) {}
}

async function toggleFlightRecorder() {
  const badge = document.getElementById('rec-status-badge');
  const isRecording = badge.textContent.includes('RECORDING') || badge.textContent.includes('AUFZEICHNUNG') || badge.textContent.includes('SPELAR');

  if (isRecording) {
    await fetch('/api/v1/recorder/stop', { method: 'POST' });
  } else {
    const customName = document.getElementById('rec-filename').value.trim();
    await fetch('/api/v1/recorder/start', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ filename: customName ? (customName.endsWith('.csv') ? customName : customName + '.csv') : null })
    });
  }
  await pollRecorderStatus();
}

// Routines Controller
async function executeRoutineEnvelope(targetModule, subFn, routineId, optionBytes, routineName) {
  const logBox = document.getElementById('routine-logs');
  const payloadBytes = [subFn, (routineId >> 8) & 0xFF, routineId & 0xFF, ...optionBytes];
  const envelope = createCommandEnvelope(targetModule, 0x31, routineId, payloadBytes);

  logBox.innerHTML += `[DISPATCH] 0x31 Routine 0x${routineId.toString(16).toUpperCase()} (${routineName}) on ${targetModule}<br>`;
  logBox.innerHTML += `&nbsp;&nbsp;&gt; Command ID: ${envelope.command_id.slice(0, 8)}... | CRC32: 0x${envelope.payload_crc32.toString(16).toUpperCase()}<br>`;
  logBox.scrollTop = logBox.scrollHeight;

  try {
    const res = await fetch('/api/v1/routine', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ envelope })
    });
    const data = await res.json();
    if (res.ok && data.success) {
      logBox.innerHTML += `&nbsp;&nbsp;<span style="color: var(--success);">&#10003; [SUCCESS] Positive Response: ${data.status_hex}</span><br>`;
    } else {
      logBox.innerHTML += `&nbsp;&nbsp;<span style="color: var(--danger);">&#10007; [FAILED] ${data.message}</span><br>`;
    }
  } catch(err) {
    logBox.innerHTML += `&nbsp;&nbsp;<span style="color: var(--danger);">&#10007; [ERROR] Network error: ${err.message}</span><br>`;
  }
  logBox.scrollTop = logBox.scrollHeight;
}

function triggerQuickRoutine(routineIdHex, module, name) {
  const rId = parseInt(routineIdHex, 16);
  executeRoutineEnvelope(module, 1, rId, [], name);
}

function triggerCustomRoutine() {
  const mod = document.getElementById('custom-mod').value.trim() || 'EDC16';
  const rHex = document.getElementById('custom-routine-id').value.trim();
  const subFn = parseInt(document.getElementById('custom-subfn').value.trim() || '1', 10);
  const optsHex = document.getElementById('custom-opts').value.trim().replace(/\s+/g, '');

  if (!rHex) {
    alert('Please enter a routine ID (e.g. 0xFF01)');
    return;
  }
  const rId = parseInt(rHex.replace(/^0x/i, ''), 16);
  const optBytes = [];
  for (let i = 0; i < optsHex.length; i += 2) {
    optBytes.push(parseInt(optsHex.substr(i, 2), 16));
  }
  executeRoutineEnvelope(mod, subFn, rId, optBytes, `Custom 0x${rId.toString(16).toUpperCase()}`);
}

// Safe Flasher
async function startSimulatedFlash() {
  const badge = document.getElementById('flash-state-badge');
  const prog = document.getElementById('flash-progress');
  const logBox = document.getElementById('flash-logs');

  logBox.innerHTML += `[STAGING] Preparing firmware manifest and ROM image...<br>`;
  const payload = {
    manifest: {
      target_module: "EDC16",
      expected_hw_id: "0281012224",
      expected_sw_id: "1037372332",
      flash_start_address: 262144,
      flash_length: 2097152,
      block_size: 4096
    },
    rom_base64: "dummy_rom_data"
  };

  const res = await fetch('/api/v1/flash/stage', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload)
  });
  const data = await res.json();
  logBox.innerHTML += `[WORKER] ${data.message}<br>`;

  const interval = setInterval(async () => {
    const pres = await fetch('/api/v1/flash/progress');
    const pdata = await pres.json();
    badge.textContent = pdata.state;
    prog.style.width = pdata.percentage + '%';
    logBox.innerHTML += `[${pdata.state}] ${pdata.log}<br>`;
    logBox.scrollTop = logBox.scrollHeight;
    if (pdata.percentage >= 100 || pdata.state === 'COMPLETED' || pdata.state === 'FAILED') {
      clearInterval(interval);
    }
  }, 500);
}

// Daimler ECU & CBF Catalog Explorer
async function searchCbfPreset(q) {
  document.getElementById('cbf-search-input').value = q;
  await searchCbf();
}

function handleCbfSearch(e) {
  if (e.key === 'Enter') {
    searchCbf();
  }
}

async function searchCbf() {
  const q = document.getElementById('cbf-search-input').value.trim();
  const tbody = document.getElementById('cbf-results-table');
  tbody.innerHTML = `<tr><td colspan="7" style="text-align: center; color: var(--text-muted);">${i18n.t('cbf.searching')}</td></tr>`;
  try {
    const res = await fetch(`/api/v1/cbf/search?q=${encodeURIComponent(q)}&limit=15`);
    const data = await res.json();
    if (!data.results || data.results.length === 0) {
      tbody.innerHTML = `<tr><td colspan="7" style="text-align: center; color: var(--warning);">${i18n.t('cbf.no_matches')}</td></tr>`;
      return;
    }
    tbody.innerHTML = data.results.map(r => `
      <tr>
        <td><b style="color: var(--accent);">${r.ecu_name}</b></td>
        <td><span class="badge" style="background: rgba(188, 140, 255, 0.15); color: var(--purple); border: 1px solid var(--purple);">${r.protocol}</span></td>
        <td><code>${r.tx_id || 'N/A'} / ${r.rx_id || 'N/A'}</code></td>
        <td>${r.date}</td>
        <td>${r.total_copies} (${r.distinct_versions} ver)</td>
        <td>${r.dtc_count}</td>
        <td><button class="btn" style="padding: 0.2rem 0.5rem; font-size: 0.75rem;" onclick="inspectCbf('${r.ecu_name}')">${i18n.t('cbf.btn_inspect')}</button></td>
      </tr>
    `).join('');
  } catch (err) {
    tbody.innerHTML = `<tr><td colspan="7" style="color: var(--danger); text-align: center;">Error searching catalog: ${err.message}</td></tr>`;
  }
}

async function inspectCbf(ecu) {
  const modal = document.getElementById('cbf-inspect-modal');
  const title = document.getElementById('inspect-title');
  const content = document.getElementById('inspect-content');
  modal.style.display = 'block';
  title.textContent = `${i18n.t('cbf.inspect_title')} ${ecu}`;
  content.innerHTML = i18n.t('cbf.loading');
  try {
    const res = await fetch(`/api/v1/cbf/inspect/${encodeURIComponent(ecu)}`);
    const data = await res.json();
    const canon = data.canonical_version;
    const chassisList = data.all_chassis_supported ? data.all_chassis_supported.join(', ') : 'None';
    content.innerHTML = `
      <div style="display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 0.75rem; margin-bottom: 0.75rem;">
        <div><b>Protocol:</b> ${canon.protocol}</div>
        <div><b>CAN Arbitration:</b> Tx: ${canon.tx_id || 'N/A'}, Rx: ${canon.rx_id || 'N/A'}</div>
        <div><b>Canonical Date:</b> ${canon.date} (${(canon.size_bytes / 1024).toFixed(1)} KB)</div>
        <div><b>Diagnostic Tables:</b> ${canon.presentation_count} presentations, ${canon.dtc_count} DTCs</div>
        <div><b>Deduplication:</b> ${data.total_copies_in_cbf} copies (${data.distinct_versions_count} distinct version(s))</div>
        <div><b>Archive Path:</b> <code>${canon.primary_path}</code></div>
      </div>
      <div style="margin-top: 0.5rem;"><b>Supported Chassis Folders (${data.all_chassis_supported ? data.all_chassis_supported.length : 0}):</b> <span style="color: var(--text-muted);">${chassisList}</span></div>
    `;
  } catch (err) {
    content.innerHTML = `<span style="color: var(--danger);">Error inspecting ECU: ${err.message}</span>`;
  }
}

function closeInspect() {
  document.getElementById('cbf-inspect-modal').style.display = 'none';
}

// Initial triggers
document.addEventListener('DOMContentLoaded', () => {
  setupTelemetryWebSocket();
  pollRecorderStatus();
});
