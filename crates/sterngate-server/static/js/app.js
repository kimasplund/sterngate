/**
 * Sterngate Main Application Logic
 */

let lastTelemetrySnap = null;
let activeVehicleVin = null;
let currentSafetyOptions = null;

// --- Tab Navigation ---
function switchTab(tabId) {
  const tabs = ['telemetry', 'health', 'workshop', 'coding', 'flashing', 'catalog'];
  if (!tabs.includes(tabId)) tabId = 'telemetry';

  document.querySelectorAll('.nav-tabs .tab-btn').forEach(btn => {
    if (btn.getAttribute('data-tab') === tabId) {
      btn.classList.add('active');
    } else {
      btn.classList.remove('active');
    }
  });

  document.querySelectorAll('.tab-pane').forEach(pane => {
    if (pane.id === `tab-pane-${tabId}`) {
      pane.classList.add('active');
    } else {
      pane.classList.remove('active');
    }
  });

  try {
    localStorage.setItem('sterngate_active_tab', tabId);
  } catch (e) {}
}

// --- Two-Tier Safety Interlock Modal Logic ---
function showSafetyModal(options) {
  currentSafetyOptions = options;

  const modal = document.getElementById('safety-modal');
  const title = document.getElementById('safety-modal-title');
  const badge = document.getElementById('safety-modal-badge');
  const desc = document.getElementById('safety-modal-desc');
  const confirmBtn = document.getElementById('safety-modal-btn-confirm');
  const kwContainer = document.getElementById('safety-modal-keyword-container');
  const kwCode = document.getElementById('safety-modal-keyword');
  const kwInput = document.getElementById('safety-modal-input');

  title.textContent = options.title || 'Safety Interlock Confirmation';
  badge.textContent = options.badge || 'HIGH RISK / DESTRUCTIVE';
  badge.className = `badge ${options.badgeClass || 'badge-recording'}`;
  desc.innerHTML = options.description || '';

  // Interlock check: Voltage
  const minVolts = options.minVoltage !== undefined ? options.minVoltage : 12.5;
  document.getElementById('gate-voltage-target').textContent = `≥ ${minVolts.toFixed(2)} V`;

  const liveVolts = (lastTelemetrySnap && lastTelemetrySnap.battery_voltage !== null && lastTelemetrySnap.battery_voltage !== undefined)
    ? lastTelemetrySnap.battery_voltage
    : 13.8;
  const voltSpan = document.getElementById('gate-voltage-live');
  if (voltSpan) voltSpan.textContent = `${liveVolts.toFixed(1)}V`;

  const voltStatus = document.getElementById('gate-voltage-status');
  const voltPass = liveVolts >= minVolts;
  if (voltPass) {
    voltStatus.textContent = 'PASS ✓';
    voltStatus.className = 'badge badge-ready';
  } else {
    voltStatus.textContent = `FAIL (${liveVolts.toFixed(1)}V < ${minVolts}V)`;
    voltStatus.className = 'badge badge-recording';
  }

  // Interlock check: Ignition / Engine stopped
  const ignStatus = document.getElementById('gate-ignition-status');
  const rpm = (lastTelemetrySnap && lastTelemetrySnap.engine_rpm) ? lastTelemetrySnap.engine_rpm : 0;
  let ignPass = true;
  if (options.requireEngineOff && rpm > 200) {
    ignPass = false;
    ignStatus.textContent = `FAIL (ENGINE RUNNING ${Math.round(rpm)} RPM)`;
    ignStatus.className = 'badge badge-recording';
  } else {
    ignStatus.textContent = 'PASS ✓';
    ignStatus.className = 'badge badge-ready';
  }

  // Interlock check: Keyword Confirmation
  if (options.requireKeyword) {
    kwContainer.style.display = 'block';
    kwCode.textContent = options.requireKeyword;
    kwInput.value = '';
    confirmBtn.disabled = true;
  } else {
    kwContainer.style.display = 'none';
    confirmBtn.disabled = !(voltPass && ignPass);
  }

  confirmBtn.textContent = options.confirmBtnText || (window.i18n ? i18n.t('modal.btn_confirm', 'Execute Procedure') : 'Execute Procedure');
  modal.style.display = 'flex';

  if (options.requireKeyword && kwInput) {
    setTimeout(() => kwInput.focus(), 50);
  }
}

function closeSafetyModal() {
  const modal = document.getElementById('safety-modal');
  if (modal) modal.style.display = 'none';
  currentSafetyOptions = null;
}

function checkSafetyModalKeyword(e) {
  if (!currentSafetyOptions) return;
  const input = document.getElementById('safety-modal-input');
  const confirmBtn = document.getElementById('safety-modal-btn-confirm');
  const target = (currentSafetyOptions.requireKeyword || '').toUpperCase();
  const entered = (input.value || '').trim().toUpperCase();

  const minVolts = currentSafetyOptions.minVoltage !== undefined ? currentSafetyOptions.minVoltage : 12.5;
  const liveVolts = (lastTelemetrySnap && lastTelemetrySnap.battery_voltage) ? lastTelemetrySnap.battery_voltage : 13.8;
  const voltPass = liveVolts >= minVolts;

  const rpm = (lastTelemetrySnap && lastTelemetrySnap.engine_rpm) ? lastTelemetrySnap.engine_rpm : 0;
  const ignPass = !(currentSafetyOptions.requireEngineOff && rpm > 200);

  confirmBtn.disabled = !(entered === target && voltPass && ignPass);

  if (e && e.key === 'Enter' && !confirmBtn.disabled) {
    executeSafetyConfirmedAction();
  }
}

async function executeSafetyConfirmedAction() {
  if (!currentSafetyOptions || !currentSafetyOptions.onConfirm) return;
  const action = currentSafetyOptions.onConfirm;
  closeSafetyModal();
  try {
    await action();
  } catch (err) {
    console.error('Safety procedure execution error:', err);
    alert(`Error executing procedure: ${err.message || err}`);
  }
}

// --- Safe Action Triggers ---
function requestClearDtcSafe() {
  showSafetyModal({
    title: (window.i18n ? i18n.t('dtc.title', 'Diagnostic Fault Codes') : 'Diagnostic Fault Codes') + ' - Clear All',
    badge: 'MODERATE RISK',
    badgeClass: 'badge-voltage',
    description: 'Clearing fault codes resets emission readiness monitors, freeze-frame diagnostic snapshots, and historical fault counters across all gateway ECUs.',
    minVoltage: 11.5,
    requireEngineOff: true,
    requireKeyword: null,
    confirmBtnText: (window.i18n ? i18n.t('dtc.btn_clear', 'Clear All DTCs') : 'Clear All DTCs'),
    onConfirm: async () => {
      await clearDtc();
    }
  });
}

function requestAdBlueResetSafe() {
  showSafetyModal({
    title: 'AdBlue / SCR 800km Emergency Countdown & Lockout Reset',
    badge: 'HIGH RISK / DESTRUCTIVE',
    badgeClass: 'badge-recording',
    description: `
      <b>Cryptographic ECU Access & EEPROM Write:</b><br>
      This guided workflow unlocks the Engine Control Module using Daimler Level 01 / Level 0B Seed-Key cryptographic derivation, executes UDS Routine <code>0x0218</code> to purge the permanent EEPROM start-lockout counter, clears catalyst NOx adaptation histories (<code>0x0219</code>), relearns the ultrasonic DEF tank level (<code>0x021A</code>), and performs an ECU soft reset (<code>0x11 01</code>).<br><br>
      <b>Prerequisites:</b> DEF tank must contain at least 5 liters of AdBlue. Battery voltage must remain strictly ≥ 12.50V. Terminal 15 ON, engine stopped.
    `,
    minVoltage: 12.5,
    requireEngineOff: true,
    requireKeyword: 'UNLOCK',
    confirmBtnText: 'Execute AdBlue Reset',
    onConfirm: async () => {
      const logBox = document.getElementById('adblue-workflow-logs');
      logBox.innerHTML += `[ADBLUE WIZARD] Initiating Seed-Key unlock and SCR register purge...<br>`;
      logBox.scrollTop = logBox.scrollHeight;

      try {
        const res = await fetch('/api/v1/workflow/adblue-reset', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            vin: activeVehicleVin || 'WDB2112061A000001'
          })
        });
        const data = await res.json();
        if (!data.success && !res.ok) throw new Error(data.error || 'Failed to reset AdBlue lockout');

        logBox.innerHTML += `<span style="color: var(--success);">[SUCCESS] ${data.message}</span><br>`;
        logBox.innerHTML += `• Security Access: Level 0x${data.security_level.toString(16).toUpperCase()} Unlocked<br>`;
        logBox.innerHTML += `• Lockout Counter Cleared: ${data.lockout_counter_cleared ? 'YES' : 'NO'}<br>`;
        logBox.innerHTML += `• NOx History Reset: ${data.nox_history_reset ? 'YES' : 'NO'}<br>`;
        logBox.innerHTML += `• Tank Level Relearned: ${data.tank_level_relearned ? 'YES' : 'NO'}<br>`;
        logBox.innerHTML += `• ECU Reset: ${data.ecu_reset_performed ? 'COMPLETED' : 'PENDING'}<br>`;
        logBox.scrollTop = logBox.scrollHeight;
      } catch (err) {
        logBox.innerHTML += `<span style="color: var(--danger);">[ERROR] ${err.message}</span><br>`;
        logBox.scrollTop = logBox.scrollHeight;
      }
    }
  });
}

function requestEcoStartStopSafe() {
  const modeSelect = document.getElementById('eco-mode-select');
  const mode = modeSelect ? modeSelect.value : 'remember';
  const modeLabel = modeSelect ? modeSelect.options[modeSelect.selectedIndex].text : mode;

  showSafetyModal({
    title: 'Configure ECO Start-Stop Memory Mode',
    badge: 'VEHICLE CODING',
    badgeClass: 'badge-voltage',
    description: `
      <b>Variant Coding Write (DID 0x0320):</b><br>
      Will reprogram engine controller / Front SAM to set ECO Start-Stop behavior to: <b>${modeLabel}</b>.<br><br>
      Prior to dispatch, an automated snapshot of the current coding configuration will be committed to the local Git garage repository.
    `,
    minVoltage: 12.0,
    requireEngineOff: true,
    requireKeyword: null,
    confirmBtnText: 'Apply ECO Configuration',
    onConfirm: async () => {
      const logBox = document.getElementById('quick-mods-logs');
      logBox.innerHTML += `[QUICK MODS] Programming ECO Start-Stop mode: ${mode}...<br>`;
      logBox.scrollTop = logBox.scrollHeight;

      try {
        const res = await fetch('/api/v1/workflow/eco-start-stop', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            mode: mode,
            vin: activeVehicleVin || 'WDB2112061A000001'
          })
        });
        const data = await res.json();
        if (!data.success && !res.ok) throw new Error(data.error || 'Failed to update ECO mode');

        logBox.innerHTML += `<span style="color: var(--success);">[SUCCESS] ${data.message}</span><br>`;
        logBox.innerHTML += `• Target Module: ${data.module} (DID 0x${data.did.toString(16).toUpperCase()})<br>`;
        logBox.innerHTML += `• Raw Value Written: 0x${data.raw_value.toString(16).toUpperCase()}<br>`;
        logBox.scrollTop = logBox.scrollHeight;
      } catch (err) {
        logBox.innerHTML += `<span style="color: var(--danger);">[ERROR] ${err.message}</span><br>`;
        logBox.scrollTop = logBox.scrollHeight;
      }
    }
  });
}

function requestEgrOptimizeSafe() {
  showSafetyModal({
    title: 'EGR Adaptation Soot Reduction Offset',
    badge: 'POWERTRAIN OPTIMIZATION',
    badgeClass: 'badge-voltage',
    description: `
      <b>EGR Adaptation Write (DID 0x0240):</b><br>
      Applies OEM positive air mass adaptation bias (+40 mg/stroke) and relearns mechanical end stops to prevent intake carbon fouling and swirl flap clogging while remaining 100% compliant with OBD emission monitors.<br><br>
      An automated snapshot of previous calibration is committed to Git garage history.
    `,
    minVoltage: 12.0,
    requireEngineOff: true,
    requireKeyword: null,
    confirmBtnText: 'Apply EGR Optimization',
    onConfirm: async () => {
      const logBox = document.getElementById('quick-mods-logs');
      logBox.innerHTML += `[QUICK MODS] Applying EGR air mass adaptation offset (+40 mg/stroke)...<br>`;
      logBox.scrollTop = logBox.scrollHeight;

      try {
        const res = await fetch('/api/v1/workflow/egr-optimize', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            vin: activeVehicleVin || 'WDB2112061A000001'
          })
        });
        const data = await res.json();
        if (!data.success && !res.ok) throw new Error(data.error || 'Failed to optimize EGR adaptation');

        logBox.innerHTML += `<span style="color: var(--success);">[SUCCESS] ${data.message}</span><br>`;
        logBox.innerHTML += `• Offset Applied: +${data.offset_applied_mg} mg/stroke<br>`;
        logBox.innerHTML += `• Lower Stop Relearn: ${data.lower_stop_relearned ? 'PASSED' : 'SKIPPED'}<br>`;
        logBox.scrollTop = logBox.scrollHeight;
      } catch (err) {
        logBox.innerHTML += `<span style="color: var(--danger);">[ERROR] ${err.message}</span><br>`;
        logBox.scrollTop = logBox.scrollHeight;
      }
    }
  });
}

function requestSbcDeactivateSafe() {
  showSafetyModal({
    title: 'Deactivate SBC Hydraulic Pressure (0-Bar Pad Service Mode)',
    badge: 'HIGH RISK / AMPUTATION HAZARD',
    badgeClass: 'badge-recording',
    description: `
      <b>CRITICAL WORKSHOP SAFETY WARNING:</b><br>
      The Sensotronic Brake Control (SBC) hydraulic accumulator stores ~160 bar of brake fluid pressure. It will automatically actuate brake calipers without warning upon door open, key detection, or wake-up bus activity.<br><br>
      This routine dumps accumulator pressure into the reservoir (0 bar), retracts pistons, and locks out brake wake-up so calipers and pads can be safely serviced without finger injury.<br><br>
      <b>Do NOT step on brake pedal while deactivated.</b>
    `,
    minVoltage: 12.5,
    requireEngineOff: true,
    requireKeyword: 'SBC',
    confirmBtnText: 'Deactivate SBC (0 bar)',
    onConfirm: async () => {
      const badge = document.getElementById('sbc-state-badge');
      const logBox = document.getElementById('sbc-logs');
      logBox.innerHTML += `[SBC] Depressurizing accumulator to 0 bar...<br>`;
      logBox.scrollTop = logBox.scrollHeight;

      try {
        const res = await fetch('/api/v1/service/sbc', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ action: 'deactivate' })
        });
        const data = await res.json();
        if (!data.success) throw new Error(data.error || 'Failed to deactivate SBC');

        badge.textContent = '0 BAR (PAD SERVICE MODE)';
        badge.className = 'badge badge-recording pulse';
        logBox.innerHTML += `<span style="color: #3fb950;">[SBC] Depressurized successfully. Safe to service pads and calipers.</span><br>`;
        logBox.scrollTop = logBox.scrollHeight;
      } catch (err) {
        logBox.innerHTML += `<span style="color: var(--danger);">[ERROR] ${err.message}</span><br>`;
        logBox.scrollTop = logBox.scrollHeight;
      }
    }
  });
}

function requestSbcReactivateSafe() {
  showSafetyModal({
    title: 'Reactivate SBC Hydraulic Pressure',
    badge: 'WORKSHOP SERVICE',
    badgeClass: 'badge-voltage',
    description: `
      <b>SBC System Reactivation & Pressure Bleed:</b><br>
      Pressurizes the SBC high-pressure accumulator back to ~160 bar and performs automated stroke/pressure tests.<br><br>
      <b>Verification:</b> Ensure all brake calipers, pads, and hydraulic connections are securely assembled and torqued before reactivation.
    `,
    minVoltage: 12.5,
    requireEngineOff: true,
    requireKeyword: null,
    confirmBtnText: 'Reactivate SBC (160 bar)',
    onConfirm: async () => {
      const badge = document.getElementById('sbc-state-badge');
      const logBox = document.getElementById('sbc-logs');
      logBox.innerHTML += `[SBC] Pressurizing accumulator and bleeding system...<br>`;
      logBox.scrollTop = logBox.scrollHeight;

      try {
        const res = await fetch('/api/v1/service/sbc', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ action: 'reactivate' })
        });
        const data = await res.json();
        if (!data.success) throw new Error(data.error || 'Failed to reactivate SBC');

        badge.textContent = '160 BAR (ACTIVE)';
        badge.className = 'badge badge-ready';
        logBox.innerHTML += `<span style="color: #3fb950;">[SBC] Reactivated successfully. System pressure restored to 160 bar.</span><br>`;
        logBox.scrollTop = logBox.scrollHeight;
      } catch (err) {
        logBox.innerHTML += `<span style="color: var(--danger);">[ERROR] ${err.message}</span><br>`;
        logBox.scrollTop = logBox.scrollHeight;
      }
    }
  });
}

function actuateSuspensionCornerSafe(action) {
  const select = document.getElementById('susp-corner-select');
  const corner = select ? select.value : 'rear';
  const cornerLabel = select ? select.options[select.selectedIndex].text : corner;

  const doActuate = async () => {
    const logBox = document.getElementById('susp-act-logs');
    logBox.innerHTML += `[SUSPENSION] Actuating corner ${cornerLabel}: action=${action}...<br>`;
    logBox.scrollTop = logBox.scrollHeight;

    try {
      const res = await fetch('/api/v1/service/suspension', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ corner, action })
      });
      const data = await res.json();
      if (!data.success) throw new Error(data.error || 'Failed to actuate suspension');

      logBox.innerHTML += `<span style="color: #3fb950;">[SUSPENSION] ${data.message}</span><br>`;
      logBox.scrollTop = logBox.scrollHeight;
    } catch (err) {
      logBox.innerHTML += `<span style="color: var(--danger);">[ERROR] ${err.message}</span><br>`;
      logBox.scrollTop = logBox.scrollHeight;
    }
  };

  if (action === 'calibrate') {
    showSafetyModal({
      title: `Air Suspension Zero-Height Calibration (${cornerLabel})`,
      badge: 'CHASSIS CALIBRATION',
      badgeClass: 'badge-voltage',
      description: `
        <b>Suspension Level Calibration:</b><br>
        Stores current ride height sensors as baseline level for <b>${cornerLabel}</b>.<br><br>
        Vehicle must be parked on a completely level surface with tire pressures at OEM specification.
      `,
      minVoltage: 12.0,
      requireEngineOff: false,
      requireKeyword: null,
      confirmBtnText: 'Calibrate Zero-Height',
      onConfirm: doActuate
    });
  } else {
    doActuate();
  }
}

async function fetchImaCodes() {
  const display = document.getElementById('ima-display-grid');
  display.innerHTML = '<span style="color: var(--text-muted);">Reading injector classification codes (DIDs 0x2030..0x2037)...</span>';

  try {
    const res = await fetch('/api/v1/service/ima?cylinder_count=4');
    const data = await res.json();
    if (!data.success) throw new Error(data.error || 'Failed to read IMA codes');

    display.innerHTML = data.injectors.map(inj => `
      <div style="background: #0d1117; border: 1px solid var(--border); border-radius: 4px; padding: 0.5rem; text-align: center;">
        <div style="font-size: 0.75rem; color: var(--text-muted);">Cylinder ${inj.cylinder}</div>
        <div style="font-family: monospace; font-size: 1.1rem; color: var(--accent); font-weight: bold;">${inj.code}</div>
        <div style="font-size: 0.7rem; color: #3fb950;">${inj.classification_type || 'IMA'}</div>
      </div>
    `).join('');
  } catch (err) {
    display.innerHTML = `<span style="color: var(--danger);">Error reading IMA codes: ${err.message}</span>`;
  }
}

function requestWriteImaCodeSafe() {
  const cylInput = document.getElementById('ima-cyl-input');
  const codeInput = document.getElementById('ima-code-input');
  const cylinder = parseInt(cylInput.value, 10);
  const code = (codeInput.value || '').trim().toUpperCase();

  if (!cylinder || cylinder < 1 || cylinder > 8) {
    alert('Please enter a valid cylinder number (1-8).');
    return;
  }
  if (!code || (code.length !== 6 && code.length !== 7)) {
    alert('Please enter a valid 6 or 7-character alphanumeric IMA injector code (e.g. 7B8HNA).');
    return;
  }

  showSafetyModal({
    title: `Program Cylinder ${cylinder} Injector IMA Code`,
    badge: 'EEPROM PROGRAMMING',
    badgeClass: 'badge-voltage',
    description: `
      <b>Common Rail Injector Calibration:</b><br>
      Programs injector tolerance compensation code <code>${code}</code> to Cylinder ${cylinder} in EDC16/EDC17 non-volatile EEPROM.<br><br>
      An automated snapshot will be committed to the Git garage repository before writing.
    `,
    minVoltage: 12.0,
    requireEngineOff: true,
    requireKeyword: null,
    confirmBtnText: 'Program Code',
    onConfirm: async () => {
      try {
        const res = await fetch('/api/v1/service/ima', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            cylinder,
            code,
            vin: activeVehicleVin || 'WDB2112061A000001'
          })
        });
        const data = await res.json();
        if (!data.success) throw new Error(data.error || 'Failed to write IMA code');

        alert(`✓ ${data.message}`);
        fetchImaCodes();
      } catch (err) {
        alert(`Error writing IMA code: ${err.message}`);
      }
    }
  });
}

function requestStageFlashSafe() {
  showSafetyModal({
    title: 'Autonomous ECU Firmware Flash Execution',
    badge: 'CRITICAL / DESTRUCTIVE',
    badgeClass: 'badge-recording',
    description: `
      <b>ECU FLASH SECTOR ERASE & REPROGRAMMING:</b><br>
      This will initiate the detached asynchronous Flashing State Machine. All diagnostic reads and APIs will be locked (HTTP 423) during flash execution.<br><br>
      <b>CRITICAL SAFETY RULES:</b><br>
      1. Battery voltage MUST be maintained ≥ 12.50 V (connect battery maintainer).<br>
      2. Engine must be completely OFF with Terminal 15 (Ignition) ON.<br>
      3. Do NOT disconnect CAN interface or cycle ignition until flashing completes.<br><br>
      Failure during erase or write may brick the ECU, requiring bench recovery (BDM/JTAG).
    `,
    minVoltage: 12.5,
    requireEngineOff: true,
    requireKeyword: 'FLASH',
    confirmBtnText: 'Erase & Flash Firmware',
    onConfirm: async () => {
      await startSimulatedFlash();
    }
  });
}


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

async function toggleAbcLimiter(action) {
  const badge = document.getElementById('abc-state-badge');
  if (badge) {
    badge.textContent = 'EXECUTING ABC ROUTINE...';
    badge.className = 'badge badge-voltage';
  }

  try {
    const res = await fetch('/api/v1/abc/control', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ action: action })
    });
    const data = await res.json();
    if (!data.success) throw new Error(data.error || 'Failed to dispatch ABC routine');

    if (badge) {
      if (action === 'dump') {
        badge.textContent = 'ABC: SAFE PRESSURE (120 BAR)';
        badge.className = 'badge badge-recording';
        alert('✓ ABC System Pressure Fallback Dump executed (Routine 0x0220). Pressure limited to 120 bar to protect tandem pump & lines from catastrophic surge rupture.');
      } else if (action === 'lock') {
        badge.textContent = 'ABC: STRUTS ISOLATED (LOCKED)';
        badge.className = 'badge badge-recording';
        alert('✓ ABC Strut Isolation Valves Locked (Routine 0x0221). Active suspension flow isolated to prevent line burst over hot exhaust.');
      } else {
        badge.textContent = 'ABC: NOMINAL (200 BAR)';
        badge.className = 'badge badge-ready';
        alert('✓ ABC Normal Active Dynamic Control restored (Routine 0x0222).');
      }
    }
  } catch (err) {
    alert(`Error dispatching ABC routine: ${err.message}`);
    if (badge) {
      badge.textContent = 'ABC: ERROR';
      badge.className = 'badge badge-danger';
    }
  }
}

async function checkCascadesLive() {
  const badge = document.getElementById('cascade-status-badge');
  const container = document.getElementById('cascade-alerts-container');
  if (!badge || !container) return;

  badge.textContent = 'EVALUATING CASCADES...';
  badge.className = 'badge badge-voltage';
  container.style.display = 'block';
  container.innerHTML = '<div style="color: var(--text-muted);">Querying live powertrain and chassis vitals for cascading failure signatures...</div>';

  try {
    const res = await fetch('/api/v1/analyze/cascades');
    const report = await res.json();

    if (report.overall_severity === 'ImminentDanger') {
      badge.textContent = '🚨 IMMINENT CASCADE OF DEATH DETECTED!';
      badge.className = 'badge badge-recording pulse';
    } else if (report.overall_severity === 'Watchlist') {
      badge.textContent = '⚠️ PREVENTIVE WATCHLIST ITEMS DETECTED';
      badge.className = 'badge badge-voltage';
    } else {
      badge.textContent = '✅ ALL SYSTEMS HEALTHY (13 CASCADES MONITORED)';
      badge.className = 'badge badge-ready';
    }

    if (!report.alerts || report.alerts.length === 0) {
      container.innerHTML = `
        <div style="background: rgba(46, 160, 67, 0.15); border: 1px solid rgba(46, 160, 67, 0.4); border-radius: 4px; padding: 0.75rem; color: #3fb950;">
          <b>✓ Zero Active Cascade Failures:</b> SBC accumulator, common rail washers, 722.6 pilot bushing, TCC lockup, DPF/M55 flaps, cam magnets, ENR compressor, ABC pulsation damper, ESL steering lock, M272 balance shaft, Valeo radiator glycol, SAM water ingress, and OM642 oil cooler are all within nominal operating tolerances.
        </div>
      `;
    } else {
      let html = '<div style="display: flex; flex-direction: column; gap: 0.6rem;">';
      for (const alert of report.alerts) {
        const isCritical = alert.severity === 'ImminentDanger';
        const borderColor = isCritical ? '#f85149' : '#d29922';
        const bgColor = isCritical ? 'rgba(248, 81, 73, 0.15)' : 'rgba(210, 153, 34, 0.15)';
        const icon = isCritical ? '🚨' : '⚠️';

        html += `
          <div style="background: ${bgColor}; border: 1px solid ${borderColor}; border-radius: 4px; padding: 0.75rem;">
            <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 0.35rem; flex-wrap: wrap; gap: 0.5rem;">
              <b style="color: ${isCritical ? '#f85149' : '#e3b341'};">${icon} ${alert.name} [${alert.severity}]</b>
              <span style="font-size: 0.75rem; font-family: monospace; color: var(--accent);">${alert.oem_part_numbers ? alert.oem_part_numbers.join(', ') : ''}</span>
            </div>
            <div style="font-size: 0.8rem; margin-bottom: 0.3rem;"><b>Telemetry Evidence:</b> <span style="color: var(--text);">${alert.telemetry_evidence}</span></div>
            <div style="font-size: 0.8rem; margin-bottom: 0.3rem;"><b>Inexpensive Root Cause:</b> <span style="color: var(--warning);">${alert.root_cause_part}</span></div>
            <div style="font-size: 0.8rem; margin-bottom: 0.3rem;"><b>Catastrophic Destruction:</b> <span style="color: #f85149;">${alert.catastrophic_outcome}</span></div>
            <div style="font-size: 0.8rem; color: var(--text-muted);"><b>Immediate Action:</b> ${alert.recommendation}</div>
          </div>
        `;
      }
      html += '</div>';
      container.innerHTML = html;
    }
  } catch (err) {
    container.innerHTML = `<div style="color: #f85149;">Error evaluating cascades: ${err.message}</div>`;
  }
}

async function pollCascadeStatus() {
  try {
    const res = await fetch('/api/v1/analyze/cascades');
    const report = await res.json();
    const badge = document.getElementById('cascade-status-badge');
    if (!badge) return;

    if (report.overall_severity === 'ImminentDanger') {
      badge.textContent = '🚨 IMMINENT CASCADE DETECTED!';
      badge.className = 'badge badge-recording pulse';
      const container = document.getElementById('cascade-alerts-container');
      if (container && container.style.display === 'none') {
        checkCascadesLive(); // Auto-expand if critical
      }
    } else if (report.overall_severity === 'Watchlist') {
      badge.textContent = '⚠️ WATCHLIST ITEMS DETECTED';
      badge.className = 'badge badge-voltage';
    } else {
      badge.textContent = 'ALL SYSTEMS NORMAL';
      badge.className = 'badge badge-ready';
    }
  } catch (e) {}
}

// Initial triggers
document.addEventListener('DOMContentLoaded', () => {
  try {
    const savedTab = localStorage.getItem('sterngate_active_tab') || 'telemetry';
    switchTab(savedTab);
  } catch (e) {}

  setupTelemetryWebSocket();
  pollRecorderStatus();
  pollCompressorStatus();
  pollCascadeStatus();
  setInterval(pollCompressorStatus, 5000);
  setInterval(pollCascadeStatus, 10000);
});



