const map = new maplibregl.Map({
  container: 'map',
  style: {
    version: 8,
    sources: {
      osm: {
        type: 'raster',
        tiles: ['https://server.arcgisonline.com/ArcGIS/rest/services/Ocean/World_Ocean_Base/MapServer/tile/{z}/{y}/{x}'],
        tileSize: 256,
        attribution: 'Esri, Garmin, GEBCO, NOAA NGDC, and other contributors'
      },
      nautical: {
        type: 'raster',
        tiles: ['https://server.arcgisonline.com/ArcGIS/rest/services/Ocean/World_Ocean_Reference/MapServer/tile/{z}/{y}/{x}'],
        tileSize: 256,
        attribution: '&copy; Esri'
      }
        },
    layers: [{ id: 'osm', type: 'raster', source: 'osm' }, { id: 'nautical', type: 'raster', source: 'nautical' }]
  },
  center: [-90, 30],
  zoom: 3
});

// TRACK_GEOJSON, PINGS_GEOJSON, SONAR_OVERLAYS, DETECTIONS_GEOJSON are declared in data.js
function load() {
  const trackGeo = TRACK_GEOJSON;
  const pingsGeo = PINGS_GEOJSON;
  const overlays = (typeof SONAR_OVERLAYS !== 'undefined') ? SONAR_OVERLAYS : [];
  const detectionsGeo = (typeof DETECTIONS_GEOJSON !== 'undefined') ? DETECTIONS_GEOJSON : {type:'FeatureCollection',features:[]};

  // Show detection count
  const detCount = detectionsGeo.features ? detectionsGeo.features.length : 0;
  const detCountEl = document.getElementById('detCount');
  if (detCountEl) detCountEl.textContent = detCount;
  const detToggleRow = document.getElementById('detectionsToggle')?.closest('.toggle-row');
  if (detToggleRow && detCount === 0) detToggleRow.style.display = 'none';

  console.log('[viewer] Data loaded: ' + overlays.length + ' overlays, ' +
    (trackGeo.features?.[0]?.geometry?.coordinates?.length || 0) + ' track points, ' +
    pingsGeo.features.length + ' pings');

  const coords = trackGeo.features?.[0]?.geometry?.coordinates ?? [];
  if (coords.length > 1) {
    const bounds = coords.reduce(
      (b, c) => b.extend(c),
      new maplibregl.LngLatBounds(coords[0], coords[0])
    );
    map.fitBounds(bounds, { padding: 48, duration: 0 });
  }

  const depths = pingsGeo.features.map(f => f.properties.depth_ft || 0).filter(d => d > 0);
    const maxDepth = depths.length ? Math.ceil(depths.reduce((a, b) => Math.max(a, b), 0)) : 60;

  // Hide/show sonar toggle if no overlays
  const toggleRow = document.querySelector('.toggle-row');
  if (!overlays.length && toggleRow) toggleRow.style.display = 'none';

  map.on('load', () => {
    // ── Sonar overlay strips (image sources) ──────────────────────────────
    const sonarLayerIds = [];
    console.log('[viewer] Loading ' + overlays.length + ' sonar overlays');
    for (let i = 0; i < overlays.length; i++) {
      const ov = overlays[i];
      const srcId = 'sonar-src-' + i;
      const layerId = 'sonar-layer-' + i;
      try {
        map.addSource(srcId, {
          type: 'image',
          url: ov.url,
          coordinates: ov.coordinates
        });
        map.addLayer({
          id: layerId,
          type: 'raster',
          source: srcId,
          paint: { 'raster-opacity': 0.92, 'raster-fade-duration': 0 }
        });
        sonarLayerIds.push(layerId);
      } catch (err) {
        console.error('[viewer] Failed to add overlay ' + i + ':', err, ov);
      }
    }

    // Track line
    map.addSource('track', { type: 'geojson', data: trackGeo });
    map.addLayer({
      id: 'track-line',
      type: 'line',
      source: 'track',
      paint: { 'line-color': '#ff5a36', 'line-width': 2.5 }
    });

    // Re-insert sonar layers below track line now that it exists
    for (const lid of sonarLayerIds) {
      map.moveLayer(lid, 'track-line');
    }

    // Ping depth circles
    map.addSource('pings', { type: 'geojson', data: pingsGeo });
    map.addLayer({
      id: 'pings-dots',
      type: 'circle',
      source: 'pings',
      paint: {
        'circle-radius': 4,
        'circle-color': [
          'interpolate', ['linear'], ['get', 'depth_ft'],
          0,               '#d0f0ff',
          maxDepth * 0.25, '#00aaff',
          maxDepth * 0.5,  '#00dd88',
          maxDepth * 0.75, '#ffcc00',
          maxDepth,        '#ff2200'
        ],
        'circle-opacity': 0.85,
        'circle-stroke-width': 0.5,
        'circle-stroke-color': 'rgba(0,0,0,0.25)'
      }
    });

    // Toggle sonar overlay visibility
    const toggle = document.getElementById('sonarToggle');
    if (toggle) {
      toggle.addEventListener('change', () => {
        const vis = toggle.checked ? 'visible' : 'none';
        for (const lid of sonarLayerIds) {
          map.setLayoutProperty(lid, 'visibility', vis);
        }
      });
    }

    // Toggle track line visibility
    const trackToggle = document.getElementById('trackToggle');
    if (trackToggle) {
      trackToggle.addEventListener('change', () => {
        map.setLayoutProperty('track-line', 'visibility', trackToggle.checked ? 'visible' : 'none');
      });
    }

    // Toggle depth pings visibility
    const depthToggle = document.getElementById('depthToggle');
    if (depthToggle) {
      depthToggle.addEventListener('change', () => {
        map.setLayoutProperty('pings-dots', 'visibility', depthToggle.checked ? 'visible' : 'none');
      });
    }

    map.on('click', 'pings-dots', e => {
      if (!e.features.length) return;
      const p = e.features[0].properties;
            const hardness = (p.bottom_hardness !== undefined && p.bottom_hardness !== null)
                ? `${Math.round(Number(p.bottom_hardness) * 100)}% (${p.bottom_type || 'unknown'})<br>`
                : '';
      new maplibregl.Popup()
        .setLngLat(e.lngLat)
        .setHTML(
          `<b>Ping #${p.sequence}</b><br>` +
          `Depth: <b>${p.depth_ft} ft</b> (${p.depth_m} m)<br>` +
                    `Bottom: ${hardness}` +
          `Channel: ${p.channel} &nbsp;&middot;&nbsp; Samples: ${p.sample_count}`
        )
        .addTo(map);
    });

    map.on('mouseenter', 'pings-dots', () => { map.getCanvas().style.cursor = 'pointer'; });
    map.on('mouseleave', 'pings-dots', () => { map.getCanvas().style.cursor = ''; });

    // ── Detection markers ──────────────────────────────────────────────────
    if (detectionsGeo.features && detectionsGeo.features.length > 0) {
      map.addSource('detections', { type: 'geojson', data: detectionsGeo });

      // Colour by classification
      const classColors = {
        fish: '#00ff88', baitball: '#00ddff', structure: '#ffaa00',
        debris: '#ff6600', wreck: '#ff0044'
      };
      const colorExpr = ['match', ['get', 'classification']];
      for (const [cls, col] of Object.entries(classColors)) {
        colorExpr.push(cls, col);
      }
      colorExpr.push('#ffffff'); // fallback

      map.addLayer({
        id: 'detections-circles',
        type: 'circle',
        source: 'detections',
        paint: {
          'circle-radius': ['interpolate', ['linear'], ['get', 'blob_area'],
            4, 5, 100, 8, 1000, 12, 10000, 18],
          'circle-color': colorExpr,
          'circle-opacity': 0.85,
          'circle-stroke-width': 2,
          'circle-stroke-color': '#ffffff'
        }
      });

      map.addLayer({
        id: 'detections-labels',
        type: 'symbol',
        source: 'detections',
        layout: {
          'text-field': ['get', 'classification'],
          'text-size': 10,
          'text-offset': [0, 1.5],
          'text-anchor': 'top'
        },
        paint: {
          'text-color': '#ffffff',
          'text-halo-color': 'rgba(0,0,0,0.7)',
          'text-halo-width': 1
        }
      });

      // Click popup for detections
      map.on('click', 'detections-circles', e => {
        if (!e.features.length) return;
        const p = e.features[0].properties;
        const conf = Math.round((p.confidence || 0) * 100);
        new maplibregl.Popup()
          .setLngLat(e.lngLat)
          .setHTML(
            `<b>${p.classification}</b> (${p.size_class})<br>` +
            `Size: <b>${p.width_m} m</b> wide &times; <b>${p.length_m} m</b> long` +
            ` (${(p.width_m * 3.281).toFixed(1)} &times; ${(p.length_m * 3.281).toFixed(1)} ft)<br>` +
            `Confidence: <b>${conf}%</b> &middot; Depth: ${p.depth_m} m<br>` +
            `Range: ${p.range_m} m &middot; ${p.channel_type}`
          )
          .addTo(map);
      });

      map.on('mouseenter', 'detections-circles', () => { map.getCanvas().style.cursor = 'pointer'; });
      map.on('mouseleave', 'detections-circles', () => { map.getCanvas().style.cursor = ''; });

      // Toggle visibility
      const detToggle = document.getElementById('detectionsToggle');
      if (detToggle) {
        detToggle.addEventListener('change', () => {
          const vis = detToggle.checked ? 'visible' : 'none';
          map.setLayoutProperty('detections-circles', 'visibility', vis);
          map.setLayoutProperty('detections-labels', 'visibility', vis);
        });
      }
    }
  });
}

load();
