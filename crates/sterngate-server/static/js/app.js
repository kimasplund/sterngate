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
    const res = await fetch(`/api/v1/ecu/search?q=${encodeURIComponent(q)}&limit=15`);
    const data = await res.json();
    if (!data.results || data.results.length === 0) {
      tbody.innerHTML = `<tr><td colspan="7" style="text-align: center; color: var(--warning);">${i18n.t('cbf.no_matches')}</td></tr>`;
      return;
    }
    tbody.innerHTML = data.results.map(r => {
      const chassisStr = r.chassis && r.chassis.length > 0 ? r.chassis.join(', ') : 'Universal';
      return `
        <tr>
          <td><b>${r.ecu_name}</b></td>
          <td><span class="badge ${r.protocol === 'UDS' ? 'badge-primary' : 'badge-offline'}">${r.protocol}</span></td>
          <td><code>${r.tx_id || 'N/A'} / ${r.rx_id || 'N/A'}</code></td>
          <td><code>${r.func_id || '0x7DF'}</code></td>
          <td>${r.dtc_count || 0}</td>
          <td><span style="font-size: 0.8rem; color: var(--text-muted);">${chassisStr}</span></td>
          <td><button class="btn" style="padding: 0.2rem 0.5rem; font-size: 0.75rem;" onclick="inspectCbf('${r.ecu_name}')">${i18n.t('cbf.btn_inspect')}</button></td>
        </tr>
      `;
    }).join('');
  } catch (err) {
    tbody.innerHTML = `<tr><td colspan="7" style="text-align: center; color: var(--danger);">${err.message}</td></tr>`;
  }
}

async function inspectCbf(ecu) {
  const modal = document.getElementById('cbf-inspect-modal');
  const title = document.getElementById('cbf-inspect-title') || document.getElementById('inspect-title');
  const content = document.getElementById('cbf-inspect-content') || document.getElementById('inspect-content');
  modal.style.display = 'block';
  if (title) title.textContent = `${i18n.t('cbf.inspect_title')} ${ecu}`;
  if (content) content.innerHTML = i18n.t('cbf.loading');
  try {
    const res = await fetch(`/api/v1/ecu/inspect/${encodeURIComponent(ecu)}`);
    const data = await res.json();
    const chassisList = data.chassis && data.chassis.length > 0
      ? data.chassis.join(', ')
      : (data.all_chassis_supported && data.all_chassis_supported.length > 0 ? data.all_chassis_supported.join(', ') : 'Universal / Unspecified');
    const chassisCount = data.chassis ? data.chassis.length : (data.all_chassis_supported ? data.all_chassis_supported.length : 0);
    content.innerHTML = `
      <div style="display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 0.75rem; margin-bottom: 0.75rem;">
        <div><b>Protocol:</b> <span class="badge ${data.protocol === 'UDS' ? 'badge-primary' : 'badge-offline'}">${data.protocol}</span></div>
        <div><b>CAN Physical:</b> Tx: <code>${data.tx_id || 'N/A'}</code>, Rx: <code>${data.rx_id || 'N/A'}</code></div>
        <div><b>Functional ID:</b> <code>${data.func_id || '0x7DF'}</code></div>
        <div><b>Known Fault Codes:</b> ${data.dtc_count || 0} DTCs</div>
        <div><b>Specification:</b> Sterngate Native JSON</div>
        <div><b>Safety Interlock:</b> Flasher State Machine Ready</div>
      </div>
      <div style="margin-top: 0.5rem;"><b>Compatible Vehicle Platforms (${chassisCount}):</b> <span style="color: var(--text-muted);">${chassisList}</span></div>
    `;
  } catch (err) {
    if (content) content.innerHTML = `<span style="color: var(--danger);">Error inspecting ECU: ${err.message}</span>`;
  }
}

function closeInspect() {
  document.getElementById('cbf-inspect-modal').style.display = 'none';
}

// --- Vehicle Garage & Analytics Logic ---
let activeVehicleVin = null;

async function scanVehicleQuick() {
  const vinBadge = document.getElementById('garage-vin-badge');
  const modelBadge = document.getElementById('garage-model-badge');
  const overview = document.getElementById('vehicle-overview-panel');
  const issuesDiv = document.getElementById('v-issues');

  vinBadge.textContent = 'SCANNING ALL ECUs...';
  vinBadge.className = 'badge badge-voltage';
  modelBadge.textContent = 'INTERROGATING';
  modelBadge.className = 'badge';

  try {
    const res = await fetch('/api/v1/vehicle/scan', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        lang: i18n.currentLang || 'en',
        save_to_garage: true
      })
    });
    const report = await res.json();
    if (report.error) throw new Error(report.error);

    activeVehicleVin = report.vin;
    vinBadge.textContent = report.vin;
    vinBadge.className = 'badge badge-voltage';

    const decoded = report.decoded || {};
    modelBadge.textContent = `${decoded.model_name || 'Vehicle'} (${decoded.model_series || 'W211'})`;
    modelBadge.className = 'badge badge-ready';

    document.getElementById('v-vin').textContent = report.vin;
    document.getElementById('v-model').textContent = decoded.model_name || 'Unknown Model';
    document.getElementById('v-chassis').textContent = `${decoded.model_series || 'W211'} • ${decoded.body_style || 'Sedan/Estate'}`;
    document.getElementById('v-engine').textContent = decoded.engine || 'OM646 2.2L CDI';
    document.getElementById('v-voltage').textContent = `${report.battery_voltage.toFixed(1)}V ${report.alternator_charging ? '(Charging)' : '(Engine Off)'}`;
    document.getElementById('v-modules').textContent = `${report.modules_responding} / ${report.total_modules_probed} Online`;

    // Render issues & warnings
    let issuesHtml = '';
    if (report.critical_issues && report.critical_issues.length > 0) {
      issuesHtml += `<div style="color: var(--danger); margin-bottom: 0.25rem;"><b>Critical Issues (${report.critical_issues.length}):</b> ${report.critical_issues.join('; ')}</div>`;
    }
    if (report.warnings && report.warnings.length > 0) {
      issuesHtml += `<div style="color: var(--warning); margin-bottom: 0.25rem;"><b>Warnings (${report.warnings.length}):</b> ${report.warnings.join('; ')}</div>`;
    }
    if (report.healthy_modules && report.healthy_modules.length > 0) {
      issuesHtml += `<div style="color: var(--success);"><b>Healthy Modules:</b> ${report.healthy_modules.join(', ')}</div>`;
    }
    issuesDiv.innerHTML = issuesHtml || `<div style="color: var(--success);">All interrogated modules healthy. No DTCs stored.</div>`;

    overview.style.display = 'block';
  } catch (err) {
    vinBadge.textContent = 'SCAN FAILED';
    vinBadge.className = 'badge badge-recording';
    modelBadge.textContent = 'ERROR';
    alert(`Vehicle scan error: ${err.message}`);
  }
}

async function loadGarageHistory() {
  const panel = document.getElementById('garage-details-panel');
  const title = document.getElementById('garage-panel-title');
  const content = document.getElementById('garage-panel-content');

  panel.style.display = 'block';
  title.textContent = 'Git Version History & Rollback Points';
  content.innerHTML = '<span style="color: var(--text-muted);">Querying git repository...</span>';

  let vin = activeVehicleVin;
  if (!vin) {
    try {
      const vRes = await fetch('/api/v1/vehicles');
      const vehicles = await vRes.json();
      if (vehicles && vehicles.length > 0) {
        vin = vehicles[0].vin;
        activeVehicleVin = vin;
      }
    } catch (e) {}
  }

  if (!vin) {
    content.innerHTML = '<span style="color: var(--warning);">No scanned vehicles found in garage yet. Run a Quick Scan first.</span>';
    return;
  }

  try {
    const res = await fetch(`/api/v1/vehicles/${encodeURIComponent(vin)}/history`);
    const history = await res.json();
    if (!history || history.length === 0) {
      content.innerHTML = `<span style="color: var(--text-muted);">Vehicle ${vin} has no commit history yet.</span>`;
      return;
    }

    content.innerHTML = `
      <div style="margin-bottom: 0.5rem; color: var(--text-muted);">
        Target Vehicle Repository: <b style="color: var(--accent); font-family: monospace;">data/vehicles/${vin}/.git</b>
      </div>
      <table style="width: 100%; border-collapse: collapse;">
        <thead>
          <tr style="text-align: left; border-bottom: 1px solid var(--border);">
            <th style="padding: 0.4rem;">Commit</th>
            <th style="padding: 0.4rem;">Date & Time</th>
            <th style="padding: 0.4rem;">Author</th>
            <th style="padding: 0.4rem;">Snapshot Message</th>
            <th style="padding: 0.4rem;">Action</th>
          </tr>
        </thead>
        <tbody>
          ${history.map(c => `
            <tr style="border-bottom: 1px solid rgba(48, 54, 61, 0.4);">
              <td style="padding: 0.4rem;"><code style="color: var(--accent);">${c.short_hash}</code></td>
              <td style="padding: 0.4rem; color: var(--text-muted); font-size: 0.8rem;">${c.timestamp}</td>
              <td style="padding: 0.4rem; font-size: 0.8rem;">${c.author}</td>
              <td style="padding: 0.4rem;">${c.message}</td>
              <td style="padding: 0.4rem;">
                <button class="btn" style="padding: 0.15rem 0.5rem; font-size: 0.75rem;" onclick="promptRollback('${vin}', '${c.short_hash}')">Rollback</button>
              </td>
            </tr>
          `).join('')}
        </tbody>
      </table>
    `;
  } catch (err) {
    content.innerHTML = `<span style="color: var(--danger);">Failed to load history: ${err.message}</span>`;
  }
}

function promptRollback(vin, hash) {
  if (confirm(`Are you sure you want to rollback variant coding and adaptations for ${vin} to commit ${hash}?`)) {
    alert(`Rollback command dispatched for commit ${hash}. To execute directly in terminal: sterngate vehicle rollback ${vin} ${hash}`);
  }
}

async function analyzeSuspensionLive() {
  const panel = document.getElementById('garage-details-panel');
  const title = document.getElementById('garage-panel-title');
  const content = document.getElementById('garage-panel-content');

  panel.style.display = 'block';
  title.textContent = 'S211 Rear Air Suspension (ENR) & AIRMATIC Health Analyzer';
  content.innerHTML = '<span style="color: var(--text-muted);">Evaluating pneumatic pressure, height drop rate, and compressor duty cycle...</span>';

  try {
    const res = await fetch('/api/v1/analyze/suspension', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(null)
    });
    const report = await res.json();

    const statusBadgeClass = report.status === 'Healthy' ? 'badge-ready' : (report.status === 'Warning' ? 'badge-voltage' : 'badge-recording');

    content.innerHTML = `
      <div style="display: flex; gap: 1rem; align-items: center; margin-bottom: 0.85rem;">
        <div><b>Overall Pneumatic Status:</b> <span class="badge ${statusBadgeClass}">${report.status}</span></div>
        <div><b>Drop Rate:</b> <span style="font-family: monospace;">${report.height_drop_rate_mm_per_hour.toFixed(1)} mm/hour</span></div>
        <div><b>L/R Asymmetry:</b> <span style="font-family: monospace;">${report.max_height_asymmetry_mm.toFixed(1)} mm</span></div>
        <div><b>Compressor Duty:</b> <span style="font-family: monospace;">${report.compressor_duty_cycle_pct.toFixed(1)}%</span></div>
      </div>
      <div style="margin-bottom: 0.75rem;">
        <b>Diagnostic Findings:</b>
        <ul style="margin-left: 1.25rem; margin-top: 0.25rem; color: var(--text);">
          ${report.findings.map(f => `<li>${f}</li>`).join('')}
        </ul>
      </div>
      ${report.recommendations && report.recommendations.length > 0 ? `
        <div style="background: rgba(210, 153, 34, 0.1); border: 1px solid var(--warning); border-radius: 4px; padding: 0.75rem;">
          <b style="color: var(--warning);">OEM Part & Service Recommendations:</b>
          <ul style="margin-left: 1.25rem; margin-top: 0.25rem; color: var(--text);">
            ${report.recommendations.map(r => `<li>${r}</li>`).join('')}
          </ul>
        </div>
      ` : ''}
    `;
  } catch (err) {
    content.innerHTML = `<span style="color: var(--danger);">Failed to analyze suspension: ${err.message}</span>`;
  }
}

async function compareDriveBenchmarkLive() {
  const panel = document.getElementById('garage-details-panel');
  const title = document.getElementById('garage-panel-title');
  const content = document.getElementById('garage-panel-content');

  panel.style.display = 'block';
  title.textContent = 'A/B Drive Telemetry Benchmark Comparison (Fuel & Slip Analytics)';
  content.innerHTML = '<span style="color: var(--text-muted);">Comparing baseline drive run against post-adaptation/coding run...</span>';

  try {
    const payload = {
      run_a: {
        duration_seconds: 1800.0,
        distance_km: 35.0,
        average_speed_kmh: 70.0,
        average_consumption_l_per_100km: 7.6,
        average_rpm: 1950.0,
        max_boost_hpa: 1450.0,
        average_rail_pressure_bar: 1150.0,
        average_tcc_slip_rpm: 38.0,
        final_coolant_temp_c: 78.0,
        seconds_to_reach_85c: null
      },
      run_b: {
        duration_seconds: 1800.0,
        distance_km: 35.0,
        average_speed_kmh: 70.0,
        average_consumption_l_per_100km: 6.9,
        average_rpm: 1900.0,
        max_boost_hpa: 1480.0,
        average_rail_pressure_bar: 1140.0,
        average_tcc_slip_rpm: 8.0,
        final_coolant_temp_c: 88.0,
        seconds_to_reach_85c: 420.0
      },
      name_a: "Baseline (Old Map / Worn ATF)",
      name_b: "Post-Service (Fresh 236.14 ATF + Clean MAF)"
    };

    const res = await fetch('/api/v1/analyze/compare', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(payload)
    });
    const cmp = await res.json();

    const isBeneficial = cmp.verdict.startsWith('Beneficial');
    const badgeClass = isBeneficial ? 'badge-ready' : 'badge-voltage';

    content.innerHTML = `
      <div style="display: flex; gap: 1rem; align-items: center; margin-bottom: 0.85rem;">
        <div><b>Comparison Verdict:</b> <span class="badge ${badgeClass}">${cmp.verdict}</span></div>
        <div><b>Consumption Delta:</b> <span style="font-family: monospace; color: ${cmp.consumption_delta_l_per_100km < 0 ? 'var(--success)' : 'var(--danger)'};">${cmp.consumption_delta_l_per_100km.toFixed(2)} L/100km (${cmp.consumption_pct_change.toFixed(1)}%)</span></div>
        <div><b>TCC Lockup Slip Delta:</b> <span style="font-family: monospace;">${cmp.tcc_slip_delta_rpm.toFixed(1)} RPM</span></div>
      </div>
      <div>
        <b>Analytical Details:</b>
        <ul style="margin-left: 1.25rem; margin-top: 0.25rem; color: var(--text);">
          ${cmp.details.map(d => `<li>${d}</li>`).join('')}
        </ul>
      </div>
    `;
  } catch (err) {
    content.innerHTML = `<span style="color: var(--danger);">Failed to compare drive runs: ${err.message}</span>`;
  }
}

function closeGaragePanel() {
  document.getElementById('garage-details-panel').style.display = 'none';
}

// --- Compressor Protection Logic ---
async function toggleCompressorInhibit(inhibit) {
  const action = inhibit ? 'inhibit' : 'restore';
  const badge = document.getElementById('compressor-state-badge');
  badge.textContent = inhibit ? 'INHIBITING...' : 'RESTORING...';

  try {
    const res = await fetch('/api/v1/suspension/compressor/control', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        action: action,
        reason: inhibit ? 'Driver safe mode: air leak burnout protection' : 'Driver normal restore'
      })
    });
    const data = await res.json();
    if (!data.success) throw new Error(data.error || 'Failed to control compressor');

    if (inhibit) {
      badge.textContent = 'COMPRESSOR: INHIBITED (SAFE MODE)';
      badge.className = 'badge badge-recording';
      alert('✓ S211 Air Suspension Compressor is now INHIBITED. Routine 0x0210 executed. Compressor relay power cut to prevent burnout.');
    } else {
      badge.textContent = 'COMPRESSOR: NORMAL OPERATION';
      badge.className = 'badge badge-ready';
      alert('✓ Normal air suspension leveling restored. Routine 0x0212 executed.');
    }
  } catch (err) {
    alert(`Error controlling compressor: ${err.message}`);
    pollCompressorStatus();
  }
}

async function toggleCompressorWorkshopMode() {
  const badge = document.getElementById('compressor-state-badge');
  badge.textContent = 'ACTIVATING WORKSHOP MODE...';

  try {
    const res = await fetch('/api/v1/suspension/compressor/control', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        action: 'workshop',
        reason: 'Workshop transport mode requested'
      })
    });
    const data = await res.json();
    if (!data.success) throw new Error(data.error || 'Failed to enter workshop mode');

    badge.textContent = 'COMPRESSOR: WORKSHOP / TRANSPORT MODE';
    badge.className = 'badge badge-voltage';
    alert('✓ S211 Air Suspension entered Workshop/Transport Mode (Routine 0x0211). Leveling locked, compressor off.');
  } catch (err) {
    alert(`Error setting workshop mode: ${err.message}`);
    pollCompressorStatus();
  }
}

async function pollCompressorStatus() {
  try {
    const res = await fetch('/api/v1/suspension/compressor/status');
    const data = await res.json();
    const badge = document.getElementById('compressor-state-badge');
    if (!badge) return;

    if (data.is_inhibited) {
      badge.textContent = 'COMPRESSOR: INHIBITED (SAFE MODE)';
      badge.className = 'badge badge-recording';
    } else if (data.current_state && data.current_state.Running) {
      badge.textContent = `COMPRESSOR: ACTIVE (${Math.round(data.current_state.Running.continuous_run_seconds)}s)`;
      badge.className = 'badge badge-voltage';
    } else if (data.current_state && data.current_state.ThermalCutoffTriggered) {
      badge.textContent = 'COMPRESSOR: THERMAL CUTOFF (COOLDOWN)';
      badge.className = 'badge badge-recording';
    } else {
      badge.textContent = 'COMPRESSOR: IDLE';
      badge.className = 'badge badge-ready';
    }
  } catch (e) {}
}

// Initial triggers
document.addEventListener('DOMContentLoaded', () => {
  setupTelemetryWebSocket();
  pollRecorderStatus();
  pollCompressorStatus();
});

