const invoke = window.__TAURI__?.core?.invoke;
const listen = window.__TAURI__?.event?.listen;
const openUrl = window.__TAURI__?.plugin?.opener?.openUrl;
const openPath = window.__TAURI__?.plugin?.opener?.openPath;

const state = {
  license: null,
  preflight: null,
  lastOutputDir: null,
  pendingLayoutRun: null,
  selectedLayoutId: null,
  unlistenProgress: null,
  unlistenVideo: null,
  unlistenVideoComplete: null,
  suggested: null,
  suggestedOptions: null,
};

function setText(id, value) {
  const node = document.getElementById(id);
  if (node) node.textContent = value;
}

function setConsole(id, value) {
  const node = document.getElementById(id);
  if (node) node.textContent = value;
}

function prettyJson(value) {
  return JSON.stringify(value, null, 2);
}

function showProgress(show) {
  document.getElementById("pipelineProgress")?.classList.toggle("hidden", !show);
}

function setProgress(step, pct) {
  const clamped = Math.max(0, Math.min(100, Number(pct) || 0));
  setText("progressStep", step || "Working…");
  setText("progressPct", `${clamped}%`);
  const bar = document.getElementById("progressBar");
  if (bar) bar.style.width = `${clamped}%`;
}

function showCompleteLink(outputDir) {
  const node = document.getElementById("pipelineComplete");
  if (!node || !outputDir) return;
  node.classList.remove("hidden");
  node.innerHTML = `Pipeline complete — <a href="#" id="openOutputLink">Open output folder</a>`;
  document.getElementById("openOutputLink")?.addEventListener("click", async (e) => {
    e.preventDefault();
    try {
      if (openPath) {
        await openPath(outputDir);
      } else {
        await invoke("pick_folder");
      }
    } catch (error) {
      setConsole("pipelineOutput", `Could not open folder: ${error}`);
    }
  });
}

function hideCompleteLink() {
  const node = document.getElementById("pipelineComplete");
  if (node) {
    node.classList.add("hidden");
    node.innerHTML = "";
  }
}

async function setupProgressListeners() {
  if (!listen) return;
  if (state.unlistenProgress) await state.unlistenProgress();
  if (state.unlistenVideo) await state.unlistenVideo();
  if (state.unlistenVideoComplete) await state.unlistenVideoComplete();

  state.unlistenProgress = await listen("pipeline-progress", (event) => {
    const { step, pct } = event.payload || {};
    setProgress(step, pct);
  });

  state.unlistenVideo = await listen("video-progress", (event) => {
    const { pct, frame, total } = event.payload || {};
    const label = total ? `Rendering video (frame ${frame}/${total})` : "Rendering video…";
    const mapped = 92 + Math.round((Number(pct) || 0) * 0.07);
    setProgress(label, mapped);
  });

  state.unlistenVideoComplete = await listen("video-complete", (event) => {
    const ok = event.payload?.ok;
    setProgress(ok ? "Video export complete" : "Video export failed", 100);
  });
}

function renderDepsList(containerId, report) {
  const ul = document.getElementById(containerId);
  if (!ul) return;
  ul.innerHTML = "";
  for (const item of report.items) {
    const li = document.createElement("li");
    li.className = item.satisfied ? "dep-ok" : "dep-missing";
    const status = item.satisfied ? "✔" : "✘";
    li.innerHTML = `<strong>${status} ${item.name}</strong><br><span class="muted">${item.message}</span>`;
    const actions = document.createElement("div");
    actions.className = "dep-actions";
    if (!item.satisfied && item.download_url) {
      const dl = document.createElement("button");
      dl.type = "button";
      dl.textContent = "Download page";
      dl.addEventListener("click", () => openDependency(item.id));
      actions.appendChild(dl);
    }
    if (!item.satisfied && item.can_auto_install) {
      const inst = document.createElement("button");
      inst.type = "button";
      inst.className = "primary";
      inst.textContent = "Auto-install";
      inst.addEventListener("click", () => installOne(item.id));
      actions.appendChild(inst);
    }
    if (item.install_hint && !item.satisfied) {
      const hint = document.createElement("code");
      hint.className = "install-hint";
      hint.textContent = item.install_hint;
      actions.appendChild(hint);
    }
    li.appendChild(actions);
    ul.appendChild(li);
  }
}

function applyPreflightUI(report) {
  state.preflight = report;
  const ready = report.ready;
  setText("dependencySummary", report.summary);
  setText("depsGateSummary", report.summary);
  setText("depsInlineMessage", ready ? "" : "Pipeline is locked until required items are installed.");
  setText("depsGateMessage", "");

  renderDepsList("depsList", report);
  renderDepsList("depsListInline", report);

  const gate = document.getElementById("depsGate");
  const runBtn = document.getElementById("runPipelineBtn");
  if (gate) gate.classList.toggle("hidden", ready);
  if (runBtn) runBtn.disabled = !ready;
}

async function refreshDependencies() {
  try {
    const report = await invoke("check_dependencies");
    applyPreflightUI(report);
  } catch (error) {
    setText("dependencySummary", `Dependency check failed: ${error}`);
  }
}

async function installAll() {
  setText("depsInlineMessage", "Installing… (winget may prompt for Administrator)");
  setText("depsGateMessage", "Installing…");
  try {
    const msg = await invoke("install_all_dependencies");
    setText("depsInlineMessage", msg);
    setText("depsGateMessage", msg);
    await refreshDependencies();
  } catch (error) {
    const text = String(error);
    setText("depsInlineMessage", text);
    setText("depsGateMessage", text);
    await refreshDependencies();
  }
}

async function installOne(id) {
  setText("depsInlineMessage", `Installing ${id}…`);
  try {
    const msg = await invoke("install_dependency", { id });
    setText("depsInlineMessage", msg);
    await refreshDependencies();
  } catch (error) {
    setText("depsInlineMessage", String(error));
    await refreshDependencies();
  }
}

async function openDependency(id) {
  try {
    if (openUrl) {
      const report = state.preflight;
      const item = report?.items?.find((i) => i.id === id);
      if (item?.download_url) await openUrl(item.download_url);
      return;
    }
    const msg = await invoke("open_dependency_url", { id });
    setText("depsInlineMessage", msg);
  } catch (error) {
    setText("depsInlineMessage", String(error));
  }
}

async function refreshLicense() {
  try {
    const status = await invoke("check_license");
    state.license = status;
    const buildFlavor = status.private_build ? "Private Build" : "Public Build";
    setText("buildFlavorBadge", buildFlavor);
    let summary = "License status unavailable.";
    if (status.state === "unlocked") {
      summary = status.private_build
        ? "Private build: license prompts are disabled."
        : "Full license active.";
    } else if (status.state === "trial") {
      summary = `Trial active: ${status.days_remaining} day(s) remaining.`;
    } else {
      summary = "Trial expired. Enter a valid license key to unlock SonarSniffer.";
    }
    setText("licenseSummary", summary);
    setText("licenseContact", `For a full license key contact: ${status.contact_email}`);
    setText("licenseMessage", status.private_build ? "This installer is pre-unlocked for internal use." : "Public key for current testing: 8106940539");
    const keyInput = document.getElementById("licenseKey");
    const activateButton = document.getElementById("activateLicenseBtn");
    if (keyInput) keyInput.disabled = status.private_build;
    if (activateButton) activateButton.disabled = status.private_build;
  } catch (error) {
    setText("licenseSummary", `License check failed: ${error}`);
  }
}

async function activateLicense() {
  const key = document.getElementById("licenseKey")?.value?.trim();
  if (!key) {
    setText("licenseMessage", "Enter a license key first.");
    return;
  }
  try {
    await invoke("activate_license", { key });
    setText("licenseMessage", "License activated.");
    await refreshLicense();
  } catch (error) {
    setText("licenseMessage", String(error));
  }
}

async function browseInput(targetId) {
  try {
    const result = await invoke("pick_input_file");
    if (result) {
      document.getElementById(targetId).value = result;
      await refreshSuggestedSettings();
    }
  } catch (error) {
    setConsole("pipelineOutput", `Browse failed: ${error}`);
  }
}

async function browseFolder() {
  try {
    const result = await invoke("pick_folder");
    if (result) {
      document.getElementById("outputFolder").value = result;
      await refreshSuggestedSettings();
    }
  } catch (error) {
    setConsole("pipelineOutput", `Folder browse failed: ${error}`);
  }
}

function hideSuggestedBanner() {
  document.getElementById("suggestedSettingsBanner")?.classList.add("hidden");
}

async function refreshSuggestedSettings() {
  if (!invoke) return;
  const fileName = document.getElementById("pipelineInput")?.value?.trim();
  const outputDir = document.getElementById("outputFolder")?.value?.trim();
  if (!fileName && !outputDir) {
    hideSuggestedBanner();
    return;
  }
  try {
    const suggested = await invoke("get_suggested_settings", {
      fileName: fileName || null,
      outputDir: outputDir || null,
      tier: "auto",
    });
    state.suggested = suggested;
    const banner = document.getElementById("suggestedSettingsBanner");
    const label = document.getElementById("suggestedTierLabel");
    const notes = document.getElementById("suggestedNotes");
    if (!banner || !label || !notes) return;
    const host = suggested.host || {};
    const resolved = suggested.resolvedTier || "auto";
    label.textContent = `Suggested: ${resolved} tier (${host.logicalCores || "?"} cores, ${(host.totalRamGb || 0).toFixed(0)} GB RAM)`;
    notes.textContent = (suggested.notes || []).join(" ");
    banner.classList.remove("hidden");
  } catch (error) {
    hideSuggestedBanner();
  }
}

function applySuggestedSettings() {
  const suggested = state.suggested;
  if (!suggested?.options) return;
  const o = suggested.options;
  const v = document.getElementById("enableVideo");
  const m = document.getElementById("enableMosaic");
  const c = document.getElementById("enableCurvelet");
  if (v) v.checked = Boolean(o.video);
  if (m) m.checked = Boolean(o.mosaic);
  if (c) c.checked = Boolean(o.curveletDenoise);
  state.suggestedOptions = { ...o };
  hideSuggestedBanner();
  setConsole("pipelineOutput", `Applied ${suggested.resolvedTier || "suggested"} settings.`);
}

function buildPipelineOptions(outDir, stitchLayoutId = null) {
  const selectedMap = document.querySelector(".swatch.selected")?.dataset?.map || "amber";
  const preset = document.getElementById("exportPreset")?.value || "";
  const base = state.suggestedOptions
    ? { ...state.suggestedOptions }
    : {
        video: Boolean(document.getElementById("enableVideo")?.checked),
        mosaic: Boolean(document.getElementById("enableMosaic")?.checked),
        curveletDenoise: Boolean(document.getElementById("enableCurvelet")?.checked),
        waterfall: true,
        kml: true,
        kmz: true,
        mbtiles: true,
        arcgis: true,
        webViewer: true,
        perWingTvg: true,
      };
  const opts = {
    ...base,
    outputDir: outDir,
    colormap: selectedMap,
    videoSpeedMode: document.getElementById("videoSpeedMode")?.value || "readable",
    overlayDepth: Boolean(document.getElementById("overlayDepth")?.checked),
    overlaySpeed: Boolean(document.getElementById("overlaySpeed")?.checked),
    overlayGps: Boolean(document.getElementById("overlayGps")?.checked),
  };
  if (preset) opts.exportPreset = preset;
  if (document.getElementById("enableDetection")?.checked) {
    opts.detectionMode = "basic";
  }
  const alignments = [];
  if (document.getElementById("flipPortWing")?.checked) {
    alignments.push({ channelId: 4, flip: true, invert: false });
  }
  if (document.getElementById("flipStarWing")?.checked) {
    alignments.push({ channelId: 5, flip: true, invert: false });
  }
  if (alignments.length) opts.channelAlignments = alignments;
  if (stitchLayoutId) opts.stitchLayoutId = stitchLayoutId;
  return opts;
}

function hideLayoutPicker() {
  document.getElementById("layoutPicker")?.classList.add("hidden");
  state.pendingLayoutRun = null;
  state.selectedLayoutId = null;
}

function showLayoutPicker(proposal, fileName, outDir) {
  const panel = document.getElementById("layoutPicker");
  const list = document.getElementById("layoutCandidates");
  const hint = document.getElementById("layoutPickerHint");
  const confirmBtn = document.getElementById("confirmLayoutBtn");
  if (!panel || !list || !proposal?.candidates?.length) return false;

  state.pendingLayoutRun = { fileName, outDir, proposal };
  state.selectedLayoutId = proposal.recommendedId || proposal.candidates[0]?.id;
  if (hint) {
    hint.textContent = `Auto confidence ${Math.round((proposal.autoConfidence || 0) * 100)}% — confirm the stitch layout before mosaic and video.`;
  }

  list.innerHTML = "";
  for (const c of proposal.candidates) {
    const card = document.createElement("label");
    card.className = "layout-card";
    const pct = Math.round((c.confidence || 0) * 100);
    const warn = (c.warnings || []).join("; ");
    card.innerHTML = `
      <input type="radio" name="layoutPick" value="${c.id}" ${c.id === state.selectedLayoutId ? "checked" : ""}>
      <div>
        <strong>${c.label}</strong>
        <span class="muted small"> · ${pct}% confidence</span>
        ${warn ? `<div class="muted small">${warn}</div>` : ""}
      </div>`;
    card.querySelector("input")?.addEventListener("change", (e) => {
      state.selectedLayoutId = e.target.value;
      if (confirmBtn) confirmBtn.disabled = !state.selectedLayoutId;
    });
    list.appendChild(card);
  }

  if (confirmBtn) confirmBtn.disabled = !state.selectedLayoutId;
  panel.classList.remove("hidden");
  setProgress("Layout confirmation required", 0);
  setConsole("pipelineOutput", "Pick a sidescan layout, then click Build with selected layout.");
  return true;
}

async function runPipeline(stitchLayoutId = null) {
  if (!state.preflight?.ready) {
    setConsole("pipelineOutput", "Install required dependencies first (GStreamer + WebView2 on Windows).");
    return;
  }
  const fileName = document.getElementById("pipelineInput")?.value?.trim();
  if (!fileName) {
    setConsole("pipelineOutput", "Select an input file first.");
    return;
  }
  const outDir = document.getElementById("outputFolder")?.value?.trim();
  if (!outDir) {
    setConsole("pipelineOutput", "Choose an output folder before running the pipeline.");
    return;
  }

  const options = buildPipelineOptions(outDir, stitchLayoutId);

  hideCompleteLink();
  if (!stitchLayoutId) hideLayoutPicker();
  showProgress(true);
  setProgress("Starting pipeline…", 0);
  setConsole("pipelineOutput", "Running SonarSniffer pipeline…");
  state.lastOutputDir = null;

  try {
    const result = await invoke("run_sonar_pipeline", { fileName, options });
    if (result?.layoutConfirmationRequired && result?.stitchLayout) {
      showLayoutPicker(result.stitchLayout, fileName, outDir);
      setConsole("pipelineOutput", prettyJson(result));
      return;
    }
    const outputDir = result?.outputs?.outputDir || result?.outputs?.output_dir;
    state.lastOutputDir = outputDir || null;
    hideLayoutPicker();
    setProgress(result?.videoRendering ? "Video rendering in background…" : "Complete", 100);
    setConsole("pipelineOutput", prettyJson(result));
    if (outputDir) {
      showCompleteLink(outputDir);
    }
  } catch (error) {
    setProgress("Pipeline failed", 0);
    setConsole("pipelineOutput", `Pipeline failed:\n${error}`);
  }
}

async function confirmLayoutAndRun() {
  if (!state.pendingLayoutRun || !state.selectedLayoutId) return;
  await runPipeline(state.selectedLayoutId);
}

window.addEventListener("DOMContentLoaded", async () => {
  document.getElementById("activateLicenseBtn")?.addEventListener("click", activateLicense);
  document.getElementById("refreshDepsBtn")?.addEventListener("click", refreshDependencies);
  document.getElementById("refreshDepsBtn2")?.addEventListener("click", refreshDependencies);
  document.getElementById("installAllDepsBtn")?.addEventListener("click", installAll);
  document.getElementById("installAllDepsBtnInline")?.addEventListener("click", installAll);
  document.getElementById("browsePipelineBtn")?.addEventListener("click", () => browseInput("pipelineInput"));
  document.getElementById("browseFolderBtn")?.addEventListener("click", browseFolder);
  document.getElementById("runPipelineBtn")?.addEventListener("click", () => runPipeline());
  document.getElementById("confirmLayoutBtn")?.addEventListener("click", confirmLayoutAndRun);
  document.getElementById("applySuggestedBtn")?.addEventListener("click", applySuggestedSettings);
  document.getElementById("dismissSuggestedBtn")?.addEventListener("click", hideSuggestedBanner);
  document.getElementById("pipelineInput")?.addEventListener("change", () => refreshSuggestedSettings());
  document.getElementById("outputFolder")?.addEventListener("change", () => refreshSuggestedSettings());

  document.getElementById("colormapSwatches")?.addEventListener("click", (e) => {
    const btn = e.target.closest(".swatch");
    if (!btn) return;
    document.querySelectorAll(".swatch").forEach((s) => s.classList.remove("selected"));
    btn.classList.add("selected");
  });

  document.querySelectorAll(".nav-item").forEach((btn) => {
    btn.addEventListener("click", () => {
      const section = btn.dataset.section;
      document.querySelectorAll(".nav-item").forEach((n) => n.classList.remove("active"));
      btn.classList.add("active");
      for (const el of document.querySelectorAll("[id^='section-']")) {
        if (el.id === "section-status") continue;
        el.classList.toggle("hidden", el.id !== `section-${section}`);
      }
    });
  });

  await setupProgressListeners();
  await Promise.all([refreshLicense(), refreshDependencies()]);
});
