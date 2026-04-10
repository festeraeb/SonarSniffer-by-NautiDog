const invoke = window.__TAURI__?.core?.invoke;

const state = {
  license: null,
};

function setText(id, value) {
  const node = document.getElementById(id);
  if (node) {
    node.textContent = value;
  }
}

function setConsole(id, value) {
  const node = document.getElementById(id);
  if (node) {
    node.textContent = value;
  }
}

function prettyJson(value) {
  return JSON.stringify(value, null, 2);
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
    if (keyInput) {
      keyInput.disabled = status.private_build;
    }
    if (activateButton) {
      activateButton.disabled = status.private_build;
    }
  } catch (error) {
    setText("licenseSummary", `License check failed: ${error}`);
  }
}

async function refreshDependencies() {
  try {
    const deps = await invoke("check_dependencies");
    const text = deps.gstreamer_available
      ? `GStreamer ready: ${deps.gstreamer_version || deps.message}`
      : deps.message;
    setText("dependencySummary", text);
  } catch (error) {
    setText("dependencySummary", `Dependency check failed: ${error}`);
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
    }
  } catch (error) {
    setConsole("pipelineOutput", `Folder browse failed: ${error}`);
  }
}

async function runPipeline() {
  const fileName = document.getElementById("pipelineInput")?.value?.trim();
  if (!fileName) {
    setConsole("pipelineOutput", "Select an input file first.");
    return;
  }

  const outDir = document.getElementById("outputFolder")?.value?.trim();
  const options = {
    video: Boolean(document.getElementById("enableVideo")?.checked),
    mosaic: Boolean(document.getElementById("enableMosaic")?.checked),
    curveletDenoise: Boolean(document.getElementById("enableCurvelet")?.checked),
    waterfall: true,
    kml: true,
    kmz: true,
    mbtiles: true,
    arcgis: true,
    webViewer: true,
  };
  if (outDir) {
    options.output_dir = outDir;
  }

  setConsole("pipelineOutput", "Running SonarSniffer pipeline...");
  try {
    const result = await invoke("run_sonar_pipeline", { fileName, options });
    setConsole("pipelineOutput", prettyJson(result));
  } catch (error) {
    setConsole("pipelineOutput", `Pipeline failed:\n${error}`);
  }
}

async function runSoundtiles() {
  const input = document.getElementById("soundtilesInput")?.value?.trim();
  if (!input) {
    setConsole("soundtilesOutput", "Select an input file first.");
    return;
  }

  const channel = document.getElementById("soundtilesChannel")?.value?.trim() || "auto";
  const tiles = Number(document.getElementById("soundtilesTiles")?.value || 20);
  const verbose = Boolean(document.getElementById("soundtilesVerbose")?.checked);

  setConsole("soundtilesOutput", "Running SoundTiles...");
  try {
    const result = await invoke("run_soundtiles", { input, channel, tiles, verbose });
    const combined = [
      `Executable: ${result.executable}`,
      `Exit code: ${result.exit_code}`,
      "",
      result.stdout || "<no stdout>",
      result.stderr ? `\n[stderr]\n${result.stderr}` : "",
    ].join("\n");
    setConsole("soundtilesOutput", combined);
  } catch (error) {
    setConsole("soundtilesOutput", `SoundTiles failed:\n${error}`);
  }
}

window.addEventListener("DOMContentLoaded", async () => {
  document.getElementById("activateLicenseBtn")?.addEventListener("click", activateLicense);
  document.getElementById("refreshDepsBtn")?.addEventListener("click", refreshDependencies);
  document.getElementById("browsePipelineBtn")?.addEventListener("click", () => browseInput("pipelineInput"));
  document.getElementById("browseSoundtilesBtn")?.addEventListener("click", () => browseInput("soundtilesInput"));
  document.getElementById("browseFolderBtn")?.addEventListener("click", browseFolder);
  document.getElementById("runPipelineBtn")?.addEventListener("click", runPipeline);
  document.getElementById("runSoundtilesBtn")?.addEventListener("click", runSoundtiles);

  await Promise.all([refreshLicense(), refreshDependencies()]);
});
