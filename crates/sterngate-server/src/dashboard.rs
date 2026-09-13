pub const DASHBOARD_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>Sterngate - Automotive Diagnostic & Telemetry Suite</title>
  <style>
    :root {
      --bg: #0d1117;
      --card-bg: #161b22;
      --border: #30363d;
      --text: #c9d1d9;
      --text-muted: #8b949e;
      --accent: #58a6ff;
      --success: #2ea043;
      --warning: #d29922;
      --danger: #f85149;
      --purple: #bc8cff;
    }
    * { box-sizing: border-box; margin: 0; padding: 0; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, monospace; }
    body { background: var(--bg); color: var(--text); padding: 1.5rem; min-height: 100vh; }
    header { display: flex; justify-content: space-between; align-items: center; border-bottom: 1px solid var(--border); padding-bottom: 1rem; margin-bottom: 1.5rem; }
    .brand { display: flex; align-items: center; gap: 0.75rem; }
    .brand-star { font-size: 1.75rem; color: var(--accent); }
    h1 { font-size: 1.5rem; font-weight: 600; }
    .badges { display: flex; gap: 0.5rem; }
    .badge { padding: 0.25rem 0.6rem; border-radius: 999px; font-size: 0.8rem; font-weight: 600; text-transform: uppercase; }
    .badge-online { background: rgba(46, 160, 67, 0.2); color: var(--success); border: 1px solid var(--success); }
    .badge-voltage { background: rgba(88, 166, 255, 0.2); color: var(--accent); border: 1px solid var(--accent); }
    .badge-recording { background: rgba(248, 81, 73, 0.2); color: var(--danger); border: 1px solid var(--danger); }
    .badge-ready { background: rgba(46, 160, 67, 0.25); color: #3fb950; border: 1px solid #3fb950; }
    @keyframes pulse {
      0% { opacity: 1; }
      50% { opacity: 0.4; }
      100% { opacity: 1; }
    }
    .pulse { animation: pulse 1.5s infinite ease-in-out; }
    .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(320px, 1fr)); gap: 1.25rem; margin-bottom: 1.5rem; }
    .card { background: var(--card-bg); border: 1px solid var(--border); border-radius: 8px; padding: 1.25rem; }
    .card-title { font-size: 1rem; color: var(--text-muted); margin-bottom: 1rem; text-transform: uppercase; letter-spacing: 0.05em; display: flex; justify-content: space-between; align-items: center; }
    .gauge-grid { display: grid; grid-template-columns: repeat(2, 1fr); gap: 1rem; }
    .gauge { background: rgba(255,255,255,0.02); border: 1px solid var(--border); border-radius: 6px; padding: 1rem; text-align: center; }
    .gauge-val { font-size: 1.8rem; font-weight: 700; color: #fff; margin: 0.25rem 0; }
    .gauge-label { font-size: 0.75rem; color: var(--text-muted); }
    .highlight-target { color: var(--success); font-size: 0.75rem; margin-top: 0.25rem; }
    .cyl-bars { display: flex; flex-direction: column; gap: 0.75rem; }
    .cyl-row { display: flex; align-items: center; justify-content: space-between; font-size: 0.85rem; }
    .bar-wrap { flex: 1; margin: 0 1rem; height: 10px; background: rgba(255,255,255,0.05); border-radius: 5px; overflow: hidden; position: relative; }
    .bar-fill { height: 100%; border-radius: 5px; transition: width 0.3s ease; }
    .btn { background: #21262d; color: var(--text); border: 1px solid var(--border); padding: 0.5rem 1rem; border-radius: 6px; cursor: pointer; font-weight: 600; font-size: 0.85rem; transition: background 0.2s; }
    .btn:hover { background: #30363d; }
    .btn-danger { background: rgba(248, 81, 73, 0.2); color: var(--danger); border-color: var(--danger); }
    .btn-danger:hover { background: rgba(248, 81, 73, 0.3); }
    .btn-primary { background: #238636; color: #fff; border-color: #2ea043; }
    .btn-primary:hover { background: #2ea043; }
    .btn-group { display: flex; flex-wrap: wrap; gap: 0.5rem; margin-bottom: 0.75rem; }
    input[type="text"] { background: #0d1117; border: 1px solid var(--border); color: var(--text); padding: 0.5rem 0.75rem; border-radius: 6px; font-size: 0.85rem; }
    input[type="text"]:focus { outline: none; border-color: var(--accent); }
    table { width: 100%; border-collapse: collapse; font-size: 0.85rem; margin-top: 0.5rem; }
    th, td { text-align: left; padding: 0.5rem; border-bottom: 1px solid var(--border); }
    th { color: var(--text-muted); }
    .progress-bar { width: 100%; height: 16px; background: rgba(255,255,255,0.05); border-radius: 8px; overflow: hidden; margin: 1rem 0; border: 1px solid var(--border); }
    .progress-fill { height: 100%; width: 0%; background: linear-gradient(90deg, var(--accent), var(--success)); transition: width 0.2s; }
    .log-box { background: #000; border: 1px solid var(--border); border-radius: 6px; padding: 0.75rem; font-family: monospace; font-size: 0.8rem; height: 120px; overflow-y: auto; color: #8b949e; line-height: 1.4; }
  </style>
</head>
<body>
  <header>
    <div class="brand">
      <span class="brand-star">★</span>
      <div>
        <h1>Sterngate</h1>
        <small style="color: var(--text-muted)">Mercedes W211 / Universal Automotive Gateway</small>
      </div>
    </div>
    <div class="badges">
      <span id="badge-status" class="badge badge-online">CONNECTED (MOCK/CAN)</span>
      <span id="badge-voltage" class="badge badge-voltage">13.8V</span>
    </div>
  </header>

  <div class="grid">
    <!-- Live Telemetry Cluster -->
    <div class="card" style="grid-column: span 2;">
      <div class="card-title">
        <span>Live Powertrain Telemetry</span>
        <button class="btn" onclick="fetchTelemetry()">Refresh</button>
      </div>
      <div class="gauge-grid">
        <div class="gauge">
          <div class="gauge-label">ENGINE RPM</div>
          <div id="val-rpm" class="gauge-val">820</div>
          <div class="gauge-label">OM646 Idle Speed</div>
        </div>
        <div class="gauge">
          <div class="gauge-label">COOLANT TEMP</div>
          <div id="val-coolant" class="gauge-val">88°C</div>
          <div class="gauge-label">Target: 85–92°C</div>
        </div>
        <div class="gauge">
          <div class="gauge-label">TRANSMISSION FLUID (722.6)</div>
          <div id="val-trans-temp" class="gauge-val" style="color: var(--accent);">80°C</div>
          <div id="badge-trans-status" class="badge badge-ready pulse" style="display: inline-block; margin-top: 0.35rem; font-size: 0.7rem;">
            ★ DIPSTICK READY: 80°C (SPEC 54-65mm) ★
          </div>
          <div class="gauge-label" style="margin-top: 0.25rem;">722.6 / NAG1 ATF Level Assistant</div>
        </div>
        <div class="gauge">
          <div class="gauge-label">COMMON RAIL PRESSURE</div>
          <div id="val-rail" class="gauge-val">320.0 bar</div>
          <div class="gauge-label">EDC16 CDI Target: 300–1600 bar</div>
        </div>
        <div class="gauge">
          <div class="gauge-label">BOOST PRESSURE (MAP)</div>
          <div id="val-boost" class="gauge-val">1040 hPa</div>
          <div class="gauge-label">Atmospheric + VNT Boost</div>
        </div>
        <div class="gauge">
          <div class="gauge-label">TCC LOCKUP SLIP</div>
          <div id="val-tcc" class="gauge-val">16 RPM</div>
          <div class="gauge-label">Torque Converter Clutch</div>
        </div>
      </div>
    </div>

    <!-- Smooth Running Cylinder Balancing -->
    <div class="card">
      <div class="card-title">Cylinder Smooth Running</div>
      <div class="cyl-bars">
        <div class="cyl-row">
          <span>Cyl 1</span>
          <div class="bar-wrap"><div id="bar-cyl1" class="bar-fill" style="width: 52%; background: var(--success);"></div></div>
          <span id="val-cyl1">+0.20 mm³</span>
        </div>
        <div class="cyl-row">
          <span>Cyl 2</span>
          <div class="bar-wrap"><div id="bar-cyl2" class="bar-fill" style="width: 48%; background: var(--success);"></div></div>
          <span id="val-cyl2">-0.15 mm³</span>
        </div>
        <div class="cyl-row">
          <span>Cyl 3</span>
          <div class="bar-wrap"><div id="bar-cyl3" class="bar-fill" style="width: 46%; background: var(--success);"></div></div>
          <span id="val-cyl3">-0.32 mm³</span>
        </div>
        <div class="cyl-row">
          <span>Cyl 4</span>
          <div class="bar-wrap"><div id="bar-cyl4" class="bar-fill" style="width: 53%; background: var(--success);"></div></div>
          <span id="val-cyl4">+0.25 mm³</span>
        </div>
      </div>
      <small style="display: block; margin-top: 1rem; color: var(--text-muted);">Threshold: ±2.0 mm³/stroke. Values outside ±3.0 indicate injector wear or nozzle fouling.</small>
    </div>

    <!-- Continuous Flight Telemetry Recorder -->
    <div class="card">
      <div class="card-title">
        <span>Flight Telemetry Recorder</span>
        <span id="rec-status-badge" class="badge" style="background: #21262d; color: var(--text-muted); border: 1px solid var(--border);">IDLE</span>
      </div>
      <div style="font-size: 0.85rem; color: var(--text-muted); margin-bottom: 0.75rem;">
        Continuous 10–50Hz CSV logging for track, tow, dyno, and road testing.
      </div>
      <div style="display: flex; gap: 0.5rem; margin-bottom: 0.75rem;">
        <input id="rec-filename" type="text" placeholder="Custom log name (optional)" style="flex: 1;">
        <button id="btn-rec-toggle" class="btn btn-danger" onclick="toggleFlightRecorder()">Start Recording</button>
      </div>
      <div style="font-size: 0.8rem; color: var(--text-muted); display: flex; flex-direction: column; gap: 0.35rem;">
        <div>Active File: <span id="rec-file-path" style="color: var(--accent); font-family: monospace;">None</span></div>
        <div>Captured Rows: <b id="rec-rows-count" style="color: #fff;">0</b> | Elapsed: <span id="rec-elapsed">0s</span></div>
      </div>
    </div>

    <!-- Actuator & Diagnostic Routines -->
    <div class="card" style="grid-column: span 2;">
      <div class="card-title">
        <span>Actuator & Diagnostic Routines (UDS 0x31)</span>
        <span class="badge" style="background: rgba(188, 140, 255, 0.2); color: var(--purple); border: 1px solid var(--purple);">Zero-Trust Protected</span>
      </div>
      <div style="font-size: 0.85rem; color: var(--text-muted); margin-bottom: 0.75rem;">
        Execute vehicle routines via CommandEnvelope verification. Fast actuators prime fuel rails, reset adaptation curves, and bleed hydraulics.
      </div>
      <div class="btn-group">
        <button class="btn" onclick="triggerQuickRoutine('0xFF01', 'EDC16', 'Fuel Pump Prime & Rail Bleed')">⚡ Prime Fuel Pump (10s Bleed)</button>
        <button class="btn" onclick="triggerQuickRoutine('0x0201', 'EDC16', 'Reset NMK Injector Adaptations')">🔄 Reset NMK Adaptations</button>
        <button class="btn" onclick="triggerQuickRoutine('0x0203', 'EDC16', 'Throttle & EGR Stop Relearn')">📐 Throttle/EGR Stop Relearn</button>
        <button class="btn" onclick="triggerQuickRoutine('0x0202', 'EDC16', 'DPF Service Regeneration')">🔥 Trigger DPF Regeneration</button>
        <button class="btn" onclick="triggerQuickRoutine('0x0205', 'EDC16', 'SBC Brake Hydraulic Bleed')">🛑 SBC Brake Hydraulic Bleed</button>
      </div>
      <div style="display: flex; gap: 0.5rem; align-items: center; margin-bottom: 0.75rem;">
        <input id="custom-mod" type="text" placeholder="Module (EDC16)" value="EDC16" style="width: 100px;">
        <input id="custom-routine-id" type="text" placeholder="Routine ID (e.g. 0xFF01)" style="width: 160px;">
        <input id="custom-subfn" type="text" placeholder="SubFn (1=Start)" value="1" style="width: 110px;">
        <input id="custom-opts" type="text" placeholder="Option Hex (optional)" style="flex: 1;">
        <button class="btn btn-primary" onclick="triggerCustomRoutine()">Execute Routine</button>
      </div>
      <div id="routine-logs" class="log-box" style="height: 100px;">
        [SYSTEM] Routine controller ready. Select a quick actuator or input custom routine ID.<br>
      </div>
    </div>

    <!-- DTC Scanner -->
    <div class="card">
      <div class="card-title">
        <span>Diagnostic Fault Codes</span>
        <div>
          <button class="btn" onclick="scanDtc()">Scan</button>
          <button class="btn btn-danger" onclick="clearDtc()">Clear All</button>
        </div>
      </div>
      <table>
        <thead>
          <tr><th>Code</th><th>Module</th><th>Description</th><th>Status</th></tr>
        </thead>
        <tbody id="dtc-table">
          <tr><td>P0100</td><td>EDC16</td><td>Mass Air Flow (MAF) Circuit</td><td><span style="color: var(--warning)">Confirmed</span></td></tr>
        </tbody>
      </table>
    </div>

    <!-- Safe Flasher Staging -->
    <div class="card">
      <div class="card-title">
        <span>Autonomous Safe Flasher (Decoupled Worker)</span>
        <span id="flash-state-badge" class="badge badge-voltage">IDLE</span>
      </div>
      <div style="display: flex; gap: 1rem; align-items: center; margin-bottom: 0.75rem;">
        <button class="btn btn-primary" onclick="startSimulatedFlash()">Stage & Execute Flash</button>
      </div>
      <span style="display: block; font-size: 0.8rem; color: var(--text-muted); margin-bottom: 0.75rem;">
        Safety Gate: Battery >= 12.5V, SHA256 & CRC verified, API Lockout active
      </span>
      <div class="progress-bar">
        <div id="flash-progress" class="progress-fill"></div>
      </div>
      <div id="flash-logs" class="log-box">
        [SYSTEM] Flasher daemon ready. Waiting for staged payload.<br>
      </div>
    </div>
  </div>

  <script>
    function makeCrcTable() {
      let c;
      const table = [];
      for (let n = 0; n < 256; n++) {
        c = n;
        for (let k = 0; k < 8; k++) {
          c = ((c & 1) ? (0xEDB88320 ^ (c >>> 1)) : (c >>> 1));
        }
        table[n] = c;
      }
      return table;
    }
    const CRC_TABLE = makeCrcTable();
    function crc32(bytes) {
      let crc = 0 ^ (-1);
      for (let i = 0; i < bytes.length; i++) {
        crc = (crc >>> 8) ^ CRC_TABLE[(crc ^ bytes[i]) & 0xFF];
      }
      return (crc ^ (-1)) >>> 0;
    }

    function createCommandEnvelope(targetModule, service, did, payloadBytes) {
      return {
        command_id: (typeof crypto !== 'undefined' && crypto.randomUUID) ? crypto.randomUUID() : 'cmd-' + Date.now(),
        timestamp_ms: Date.now(),
        ttl_ms: 10000,
        idempotency_key: (typeof crypto !== 'undefined' && crypto.randomUUID) ? crypto.randomUUID() : 'idemp-' + Math.random(),
        target_module: targetModule,
        service: service,
        did: did,
        payload_len: payloadBytes.length,
        payload_crc32: crc32(payloadBytes),
        payload: Array.from(payloadBytes)
      };
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

    const ws = new WebSocket(`ws://${location.host}/ws/telemetry`);
    ws.onmessage = (e) => {
      try {
        const snap = JSON.parse(e.data);
        if (snap.engine_rpm !== null && snap.engine_rpm !== undefined) document.getElementById('val-rpm').textContent = Math.round(snap.engine_rpm);
        if (snap.coolant_temp !== null && snap.coolant_temp !== undefined) document.getElementById('val-coolant').textContent = snap.coolant_temp + '°C';
        if (snap.trans_fluid_temp !== null && snap.trans_fluid_temp !== undefined) {
          const t = snap.trans_fluid_temp;
          document.getElementById('val-trans-temp').textContent = t + '°C';
          const badge = document.getElementById('badge-trans-status');
          if (t < 78.0) {
            badge.className = 'badge';
            badge.style.background = 'rgba(88, 166, 255, 0.2)';
            badge.style.color = 'var(--accent)';
            badge.style.border = '1px solid var(--accent)';
            badge.textContent = `WARMING UP (${t}°C < 78°C) - NOT READY`;
          } else if (t <= 82.0) {
            badge.className = 'badge badge-ready pulse';
            badge.style.background = 'rgba(46, 160, 67, 0.25)';
            badge.style.color = '#3fb950';
            badge.style.border = '1px solid #3fb950';
            badge.textContent = `★ READY AT ${t}°C: MEASURE DIPSTICK (SPEC 54-65mm) ★`;
          } else {
            badge.className = 'badge';
            badge.style.background = 'rgba(210, 153, 34, 0.2)';
            badge.style.color = 'var(--warning)';
            badge.style.border = '1px solid var(--warning)';
            badge.textContent = `COOL DOWN (${t}°C > 82°C) - OVERHEAT`;
          }
        }
        if (snap.rail_pressure !== null && snap.rail_pressure !== undefined) document.getElementById('val-rail').textContent = snap.rail_pressure.toFixed(1) + ' bar';
        if (snap.boost_pressure !== null && snap.boost_pressure !== undefined) document.getElementById('val-boost').textContent = Math.round(snap.boost_pressure) + ' hPa';
        if (snap.tcc_slip_rpm !== null && snap.tcc_slip_rpm !== undefined) document.getElementById('val-tcc').textContent = Math.round(snap.tcc_slip_rpm) + ' RPM';
        if (snap.battery_voltage !== null && snap.battery_voltage !== undefined) document.getElementById('badge-voltage').textContent = snap.battery_voltage.toFixed(1) + 'V';

        updateCylBar('val-cyl1', 'bar-cyl1', snap.inj_corr_cyl1);
        updateCylBar('val-cyl2', 'bar-cyl2', snap.inj_corr_cyl2);
        updateCylBar('val-cyl3', 'bar-cyl3', snap.inj_corr_cyl3);
        updateCylBar('val-cyl4', 'bar-cyl4', snap.inj_corr_cyl4);
      } catch(err) {}
    };

    async function scanDtc() {
      const res = await fetch('/api/v1/dtc');
      const data = await res.json();
      const tbody = document.getElementById('dtc-table');
      if (data.length === 0) {
        tbody.innerHTML = '<tr><td colspan="4" style="color: var(--success); text-align: center;">No diagnostic fault codes stored.</td></tr>';
      } else {
        tbody.innerHTML = data.map(d => `<tr><td><b>${d.code}</b></td><td>${d.module}</td><td>${d.description}</td><td><span style="color: var(--warning)">Active</span></td></tr>`).join('');
      }
    }

    async function clearDtc() {
      await fetch('/api/v1/dtc/clear', { method: 'POST' });
      document.getElementById('dtc-table').innerHTML = '<tr><td colspan="4" style="color: var(--success); text-align: center;">Fault memory cleared.</td></tr>';
    }

    async function fetchTelemetry() {
      const res = await fetch('/api/v1/telemetry');
      const snap = await res.json();
      if (snap.engine_rpm) document.getElementById('val-rpm').textContent = Math.round(snap.engine_rpm);
    }

    async function triggerQuickRoutine(routineIdHex, module, name) {
      const logBox = document.getElementById('routine-logs');
      logBox.innerHTML += `[SEND] Dispatching ${name} (${routineIdHex}) to ${module}...<br>`;
      logBox.scrollTop = logBox.scrollHeight;
      const rId = parseInt(routineIdHex.replace('0x', ''), 16);
      const payloadBytes = [1, (rId >> 8) & 0xFF, rId & 0xFF];
      const envelope = createCommandEnvelope(module, 0x31, rId, payloadBytes);
      try {
        const res = await fetch('/api/v1/routine', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ envelope })
        });
        const data = await res.json();
        if (data.success) {
          logBox.innerHTML += `<span style="color: var(--success)">[SUCCESS] ${data.message} (Payload: ${data.status_hex})</span><br>`;
        } else {
          logBox.innerHTML += `<span style="color: var(--danger)">[REJECTED] ${data.message}</span><br>`;
        }
      } catch (e) {
        logBox.innerHTML += `<span style="color: var(--danger)">[ERROR] ${e.message}</span><br>`;
      }
      logBox.scrollTop = logBox.scrollHeight;
    }

    async function triggerCustomRoutine() {
      const mod = document.getElementById('custom-mod').value.trim() || 'EDC16';
      const rIdHex = document.getElementById('custom-routine-id').value.trim();
      if (!rIdHex) { alert('Please enter Routine ID hex (e.g. 0xFF01)'); return; }
      const subfn = parseInt(document.getElementById('custom-subfn').value.trim() || '1');
      const opts = document.getElementById('custom-opts').value.trim();
      const logBox = document.getElementById('routine-logs');
      logBox.innerHTML += `[SEND] Executing custom routine ${rIdHex} on ${mod}...<br>`;
      const rId = parseInt(rIdHex.replace('0x', ''), 16);
      let optBytes = [];
      if (opts) {
        for (let i = 0; i < opts.length; i += 2) {
          optBytes.push(parseInt(opts.substr(i, 2), 16) || 0);
        }
      }
      const payloadBytes = [subfn, (rId >> 8) & 0xFF, rId & 0xFF, ...optBytes];
      const envelope = createCommandEnvelope(mod, 0x31, rId, payloadBytes);
      try {
        const res = await fetch('/api/v1/routine', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ envelope })
        });
        const data = await res.json();
        if (data.success) {
          logBox.innerHTML += `<span style="color: var(--success)">[SUCCESS] ${data.message} (Resp: ${data.status_hex})</span><br>`;
        } else {
          logBox.innerHTML += `<span style="color: var(--danger)">[REJECTED] ${data.message}</span><br>`;
        }
      } catch (e) {
        logBox.innerHTML += `<span style="color: var(--danger)">[ERROR] ${e.message}</span><br>`;
      }
      logBox.scrollTop = logBox.scrollHeight;
    }

    let isRecording = false;
    async function toggleFlightRecorder() {
      if (!isRecording) {
        const filename = document.getElementById('rec-filename').value.trim() || undefined;
        const res = await fetch('/api/v1/recorder/start', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ filename })
        });
        const status = await res.json();
        updateRecorderUi(status);
      } else {
        const res = await fetch('/api/v1/recorder/stop', { method: 'POST' });
        const status = await res.json();
        updateRecorderUi(status);
      }
    }

    function updateRecorderUi(st) {
      isRecording = st.is_recording;
      const badge = document.getElementById('rec-status-badge');
      const btn = document.getElementById('btn-rec-toggle');
      const path = document.getElementById('rec-file-path');
      const rows = document.getElementById('rec-rows-count');
      const elapsed = document.getElementById('rec-elapsed');
      if (isRecording) {
        badge.className = 'badge badge-recording pulse';
        badge.style.background = 'rgba(248, 81, 73, 0.2)';
        badge.style.color = 'var(--danger)';
        badge.style.border = '1px solid var(--danger)';
        badge.textContent = `REC (${st.elapsed_seconds}s)`;
        btn.textContent = 'Stop Recording';
        btn.className = 'btn btn-primary';
      } else {
        badge.className = 'badge';
        badge.style.background = '#21262d';
        badge.style.color = 'var(--text-muted)';
        badge.style.border = '1px solid var(--border)';
        badge.textContent = 'IDLE';
        btn.textContent = 'Start Recording';
        btn.className = 'btn btn-danger';
      }
      path.textContent = st.current_file ? st.current_file.split('/').pop() : 'None';
      rows.textContent = st.records_count;
      elapsed.textContent = st.elapsed_seconds + 's';
    }

    setInterval(async () => {
      try {
        const res = await fetch('/api/v1/recorder/status');
        const st = await res.json();
        if (st.is_recording || isRecording) {
          updateRecorderUi(st);
        }
      } catch(e) {}
    }, 1000);

    async function startSimulatedFlash() {
      const logBox = document.getElementById('flash-logs');
      const prog = document.getElementById('flash-progress');
      const badge = document.getElementById('flash-state-badge');
      
      const res = await fetch('/api/v1/flash/stage', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          manifest: {
            target_module: 'EDC16',
            expected_hw_id: '0281012224',
            expected_sw_id: '1037372332',
            sha256_checksum: 'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855',
            crc32_checksum: 0,
            flash_start_address: 262144,
            flash_length: 1048576,
            block_size: 1024
          },
          rom_base64: ''
        })
      });

      const data = await res.json();
      logBox.innerHTML += `[STAGING] ${data.message}<br>`;
      
      // Poll progress
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
  </script>
</body>
</html>"#;
