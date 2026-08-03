(function () {
  'use strict';

  const canvas = document.getElementById('network-canvas');
  if (!canvas) return;

  const ctx = canvas.getContext('2d');
  const dpr = window.devicePixelRatio || 1;

  let width = 0;
  let height = 0;
  let nodes = null;
  let rain = [];
  let gridOffset = 0;

  const CYCLE = 36000;
  const CONNECT_END = 8000;
  const SEND_END = 15000;
  const RETURN_END = 33000;

  const PHI = 1.618033988749895;
  const GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5));

  const COLORS = {
    host: '#00f0ff',
    hostGlow: 'rgba(0, 240, 255, 0.45)',
    hostRing: 'rgba(0, 240, 255, 0.35)',
    jump: 'rgba(0, 240, 255, 0.55)',
    jumpGlow: 'rgba(0, 240, 255, 0.12)',
    target: 'rgba(0, 240, 255, 0.16)',
    line: 'rgba(0, 240, 255, 0.05)',
    lineActive: 'rgba(0, 240, 255, 0.10)',
    sendFlare: 'rgba(140, 230, 255, 0.55)',
    sendGlow: 'rgba(140, 230, 255, 0.18)',
    returnFlare: 'rgba(0, 210, 255, 0.55)',
    returnGlow: 'rgba(0, 210, 255, 0.18)',
    grid: 'rgba(0, 240, 255, 0.05)',
    rain: 'rgba(0, 240, 255, 0.1)',
  };

  function resize() {
    const rect = canvas.getBoundingClientRect();
    width = Math.max(1, Math.floor(rect.width));
    height = Math.max(1, Math.floor(rect.height));
    canvas.width = Math.floor(width * dpr);
    canvas.height = Math.floor(height * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    nodes = buildNodes(width, height);
    rain = buildRain(width, height);
  }

  function makeRng(seed) {
    let s = seed >>> 0;
    if (s === 0) s = 12345;
    return function () {
      s ^= s << 13;
      s ^= s >>> 17;
      s ^= s << 5;
      return (s >>> 0) / 4294967296;
    };
  }

  function phyllotaxisPoints(count, radius) {
    const points = [];
    const scale = radius / Math.sqrt(count);
    for (let i = 0; i < count; i++) {
      const r = scale * Math.sqrt(i + 0.5);
      const theta = i * GOLDEN_ANGLE;
      points.push({ x: r * Math.cos(theta), y: r * Math.sin(theta) });
    }
    return points;
  }

  function buildNodes(w, h) {
    const rng = makeRng(0xcafebabe);

    // Host above the center of the fan-out so it sits at the top of the screen.
    const host = {
      type: 'host',
      x: 0,
      y: -w * 0.18,
      z: 0,
      baseY: -w * 0.18,
      r: 4,
    };

    // 100 jump hosts in a small inner disc (x-z plane).
    const jumpCount = 100;
    const jumpRadius = Math.min(w * 0.10, h * 0.16);
    const jumps = phyllotaxisPoints(jumpCount, jumpRadius).map((p, i) => ({
      type: 'jump',
      x: p.x,
      y: 0,
      z: p.y,
      r: 1.5,
      index: i,
    }));

    // 10,000 target hosts in a large outer disc (x-z plane).
    const targetsPerJump = 100;
    const targetCount = jumpCount * targetsPerJump;
    const targetRadius = Math.min(w * 0.46, h * 0.72);
    const targetInnerRadius = jumpRadius * 4.8;
    const targets = [];

    // Distribute targets uniformly in an annulus and assign each to its nearest
    // jump host by angle, creating radial pizza-slice sectors.
    for (let i = 0; i < targetCount; i++) {
      const localIndex = i % targetsPerJump;
      const jumpIndex = Math.floor(i / targetsPerJump);
      const jumpAngle = Math.atan2(jumps[jumpIndex].z, jumps[jumpIndex].x);
      const sectorHalfWidth = Math.PI / jumpCount;

      // Uniform random radius in the annulus, biased slightly outward.
      const u = (localIndex + 0.5) / targetsPerJump;
      const r = targetInnerRadius + (targetRadius - targetInnerRadius) * Math.sqrt(u);
      // Angle near the parent jump, with a little golden-ratio jitter.
      const theta = jumpAngle + (rng() - 0.5) * sectorHalfWidth * 2.2 + localIndex * GOLDEN_ANGLE * 0.02;

      targets.push({
        type: 'target',
        x: r * Math.cos(theta),
        y: 0,
        z: r * Math.sin(theta),
        r: 0.7,
        jumpIndex: jumpIndex,
        index: i,
      });
    }

    return { host, jumps, targets };
  }

  function buildRain(w, h) {
    const rng = makeRng(0xdeadbeef);
    const drops = [];
    const count = Math.floor(w / 7);
    for (let i = 0; i < count; i++) {
      drops.push({
        x: rng() * w,
        y: rng() * h,
        speed: 0.3 + rng() * 1.0,
        length: 4 + rng() * 10,
        alpha: 0.04 + rng() * 0.12,
      });
    }
    return drops;
  }

  function easeOutCubic(t) {
    return 1 - Math.pow(1 - t, 3);
  }
  function easeInOutSine(t) {
    return -(Math.cos(Math.PI * t) - 1) / 2;
  }
  function easeInQuad(t) {
    return t * t;
  }
  function clamp(t, min, max) {
    return Math.max(min, Math.min(max, t));
  }

  // Project a 3D point to screen space. cameraY rotates around the vertical axis,
  // cameraX gives a slight downward tilt. Focal length is adaptive to canvas width.
  function project(p, cameraY, cameraX, focalLength) {
    const cosY = Math.cos(cameraY);
    const sinY = Math.sin(cameraY);
    const x1 = p.x * cosY + p.z * sinY;
    const z1 = -p.x * sinY + p.z * cosY;

    const cosX = Math.cos(cameraX);
    const sinX = Math.sin(cameraX);
    const y1 = p.y * cosX - z1 * sinX;
    const z2 = p.y * sinX + z1 * cosX;

    const scale = focalLength / (focalLength + z2);
    return {
      x: width / 2 + x1 * scale,
      y: height / 2 + y1 * scale,
      scale: scale,
      z: z2,
    };
  }

  function drawConnection(a, b, progress, color, width) {
    if (progress <= 0) return;
    const x = a.x + (b.x - a.x) * progress;
    const y = a.y + (b.y - a.y) * progress;
    ctx.beginPath();
    ctx.moveTo(a.x, a.y);
    ctx.lineTo(x, y);
    ctx.strokeStyle = color;
    ctx.lineWidth = width;
    ctx.stroke();
  }

  function drawNode(proj, color, glowColor, glowRadius, baseRadius) {
    const r = Math.max(0.4, baseRadius * proj.scale);
    if (glowColor && glowRadius > 0) {
      const gr = Math.max(2, glowRadius * proj.scale);
      const grad = ctx.createRadialGradient(proj.x, proj.y, r, proj.x, proj.y, gr);
      grad.addColorStop(0, glowColor);
      grad.addColorStop(1, 'transparent');
      ctx.beginPath();
      ctx.arc(proj.x, proj.y, gr, 0, Math.PI * 2);
      ctx.fillStyle = grad;
      ctx.fill();
    }
    ctx.beginPath();
    ctx.arc(proj.x, proj.y, r, 0, Math.PI * 2);
    ctx.fillStyle = color;
    ctx.fill();
  }

  function drawHostRing(proj, baseRadius, spinAngle) {
    const r = baseRadius * proj.scale * 2.8;
    ctx.save();
    ctx.translate(proj.x, proj.y);
    ctx.rotate(spinAngle);
    ctx.strokeStyle = COLORS.hostRing;
    ctx.lineWidth = Math.max(0.5, 1.2 * proj.scale);

    ctx.beginPath();
    ctx.arc(0, 0, r, 0, Math.PI * 2);
    ctx.stroke();

    ctx.beginPath();
    ctx.moveTo(-r * 1.4, 0);
    ctx.lineTo(r * 1.4, 0);
    ctx.moveTo(0, -r * 1.4);
    ctx.lineTo(0, r * 1.4);
    ctx.stroke();

    ctx.restore();
  }

  function drawFlare(a, b, t, color, glowColor, size) {
    const x = a.x + (b.x - a.x) * t;
    const y = a.y + (b.y - a.y) * t;
    const grad = ctx.createRadialGradient(x, y, 0, x, y, size * 3);
    grad.addColorStop(0, color);
    grad.addColorStop(0.4, glowColor);
    grad.addColorStop(1, 'transparent');
    ctx.beginPath();
    ctx.arc(x, y, size * 3, 0, Math.PI * 2);
    ctx.fillStyle = grad;
    ctx.fill();
  }

  function drawGrid(dt) {
    gridOffset = (gridOffset + dt * 0.015) % 40;

    ctx.save();
    ctx.strokeStyle = COLORS.grid;
    ctx.lineWidth = 1;

    for (let y = gridOffset; y < height; y += 40) {
      const alpha = 1 - y / height;
      ctx.globalAlpha = alpha * 0.4;
      ctx.beginPath();
      ctx.moveTo(0, y);
      ctx.lineTo(width, y);
      ctx.stroke();
    }

    ctx.globalAlpha = 0.2;
    const cx = width / 2;
    for (let x = 0; x <= width; x += 70) {
      ctx.beginPath();
      ctx.moveTo(x, 0);
      ctx.lineTo(cx + (x - cx) * 0.3, height);
      ctx.stroke();
    }

    ctx.restore();
  }

  function drawRain(dt) {
    ctx.save();
    ctx.lineWidth = 1;
    for (let i = 0; i < rain.length; i++) {
      const drop = rain[i];
      drop.y += drop.speed * dt * 0.06;
      if (drop.y > height) {
        drop.y = -drop.length;
        drop.x = Math.random() * width;
      }
      ctx.strokeStyle = `rgba(0, 240, 255, ${drop.alpha})`;
      ctx.beginPath();
      ctx.moveTo(drop.x, drop.y);
      ctx.lineTo(drop.x, drop.y + drop.length);
      ctx.stroke();
    }
    ctx.restore();
  }

  let lastTime = 0;
  function render(time) {
    if (!nodes) return;
    const dt = Math.min(50, time - lastTime);
    lastTime = time;

    ctx.clearRect(0, 0, width, height);

    drawGrid(dt);
    drawRain(dt);

    const cycleT = time % CYCLE;

    // Camera choreography over the cycle.
    // The disc is in the x-z plane; rotating around Y tilts it sideways.
    // 0-15s: side view -> top-down as connections fan out.
    // 15-33s: drift past top-down toward the opposite side during data return.
    // 33-36s: spin back to the original side and zoom in during teardown.
    let cameraY;
    const sideAngle = Math.PI / 3;
    if (cycleT < SEND_END) {
      const t = easeOutCubic(cycleT / SEND_END);
      cameraY = sideAngle * (1 - t);
    } else if (cycleT < RETURN_END) {
      const t = (cycleT - SEND_END) / (RETURN_END - SEND_END);
      cameraY = -sideAngle * easeInOutSine(t);
    } else {
      const t = (cycleT - RETURN_END) / (CYCLE - RETURN_END);
      cameraY = -sideAngle + 2 * sideAngle * easeInOutSine(t);
    }

    const cameraX = 0.18; // slight downward tilt for depth

    // Zoom: pull back while connecting to targets, push back in on teardown.
    let zoomT = 0;
    if (cycleT < SEND_END) {
      zoomT = easeOutCubic(cycleT / SEND_END);
    } else if (cycleT < RETURN_END) {
      zoomT = 1;
    } else {
      zoomT = 1 - easeInQuad((cycleT - RETURN_END) / (CYCLE - RETURN_END));
    }
    const baseFocal = width * 1.05;
    const zoomedFocal = width * 0.60;
    const focalLength = baseFocal + (zoomedFocal - baseFocal) * zoomT;

    let connectProgress = 0;
    let sendProgress = 0;
    let returnProgress = 0;
    let teardownProgress = 0;
    let wobblePhase = 0;

    if (cycleT < CONNECT_END) {
      connectProgress = cycleT / CONNECT_END;
    } else if (cycleT < SEND_END) {
      connectProgress = 1;
      sendProgress = (cycleT - CONNECT_END) / (SEND_END - CONNECT_END);
    } else if (cycleT < RETURN_END) {
      connectProgress = 1;
      sendProgress = 1;
      returnProgress = (cycleT - SEND_END) / (RETURN_END - SEND_END);
    } else {
      connectProgress = 1;
      sendProgress = 1;
      returnProgress = 1;
      teardownProgress = (cycleT - RETURN_END) / (CYCLE - RETURN_END);
      wobblePhase = teardownProgress;
    }

    // Host wobble during tear-down (model-space Y).
    const wobble = wobblePhase > 0
      ? Math.sin(wobblePhase * Math.PI * 6) * 5 * (1 - wobblePhase)
      : 0;
    nodes.host.y = nodes.host.baseY + wobble;

    // Project all nodes and mark visibility.
    function isVisible(proj) {
      return proj.scale > 0.15 && proj.z > -focalLength * 0.85;
    }
    const hostProj = project(nodes.host, cameraY, cameraX, focalLength);
    hostProj.visible = isVisible(hostProj);
    const jumpProjs = nodes.jumps.map(j => {
      const p = project(j, cameraY, cameraX, focalLength);
      p.visible = isVisible(p);
      return p;
    });
    const targetProjs = nodes.targets.map(t => {
      const p = project(t, cameraY, cameraX, focalLength);
      p.visible = isVisible(p);
      return p;
    });

    // Connection progress with connect + teardown.
    const jumpHostProgress = nodes.jumps.map((jump, i) => {
      const stagger = i / nodes.jumps.length;
      const localConnect = clamp((connectProgress - stagger * 0.6) / 0.4, 0, 1);
      const connect = easeOutCubic(localConnect);
      const localTeardown = teardownProgress > 0
        ? clamp(1 - (teardownProgress - (1 - stagger) * 0.5) / 0.5, 0, 1)
        : 1;
      const teardown = easeInQuad(localTeardown);
      return connect * teardown;
    });

    const targetProgress = nodes.targets.map((target, i) => {
      const jumpIndex = target.jumpIndex;
      const localIndex = i % 100;
      const jumpStagger = jumpIndex / nodes.jumps.length;
      const targetStagger = localIndex / 100;
      const localConnect = clamp((connectProgress - 0.25 - jumpStagger * 0.45 - targetStagger * 0.3) / 0.4, 0, 1);
      const connect = easeOutCubic(localConnect);
      const localTeardown = teardownProgress > 0
        ? clamp(1 - (teardownProgress - (1 - targetStagger) * 0.55) / 0.45, 0, 1)
        : 1;
      const teardown = easeInQuad(localTeardown);
      return connect * teardown;
    });

    // Draw host -> jump connections.
    nodes.jumps.forEach((jump, i) => {
      const p = jumpHostProgress[i];
      if (p > 0 && hostProj.visible && jumpProjs[i].visible) {
        drawConnection(hostProj, jumpProjs[i], p, COLORS.lineActive, Math.max(0.3, 1 * hostProj.scale));
      }
    });

    // Draw jump -> target connections.
    nodes.jumps.forEach((jump, ji) => {
      const jumpP = jumpHostProgress[ji];
      if (jumpP <= 0 || !jumpProjs[ji].visible) return;
      const start = ji * 100;
      const end = start + 100;
      for (let i = start; i < end; i++) {
        const p = targetProgress[i];
        if (p > 0 && targetProjs[i].visible) {
          drawConnection(jumpProjs[ji], targetProjs[i], Math.min(jumpP, p), COLORS.line, Math.max(0.25, 0.7 * jumpProjs[ji].scale));
        }
      }
    });

    // Outward data send.
    if (sendProgress > 0 && sendProgress < 1 && teardownProgress === 0) {
      const p = sendProgress;
      nodes.targets.forEach((target, i) => {
        const jumpIdx = target.jumpIndex;
        const wave = (i % 40) / 40;
        const start = wave * 0.8;
        const local = clamp((p - start) / 0.2, 0, 1);
        if (local <= 0 || local >= 1) return;
        const eased = easeInOutSine(local);
        if (eased < 0.5) {
          if (hostProj.visible && jumpProjs[jumpIdx].visible) {
            drawFlare(hostProj, jumpProjs[jumpIdx], eased * 2, COLORS.sendFlare, COLORS.sendGlow, Math.max(0.3, 0.6 * hostProj.scale));
          }
        } else {
          if (jumpProjs[jumpIdx].visible && targetProjs[i].visible) {
            drawFlare(jumpProjs[jumpIdx], targetProjs[i], (eased - 0.5) * 2, COLORS.sendFlare, COLORS.sendGlow, Math.max(0.25, 0.45 * jumpProjs[jumpIdx].scale));
          }
        }
      });
    }

    // Return data.
    if (returnProgress > 0 && teardownProgress === 0) {
      const p = returnProgress;
      nodes.targets.forEach((target, i) => {
        const jumpIdx = target.jumpIndex;
        const wave = (i % 60) / 60;
        const start = wave * 0.92;
        const local = clamp((p - start) / 0.08, 0, 1);
        if (local <= 0 || local >= 1) return;
        const eased = easeInOutSine(local);
        if (eased < 0.5) {
          if (targetProjs[i].visible && jumpProjs[jumpIdx].visible) {
            drawFlare(targetProjs[i], jumpProjs[jumpIdx], eased * 2, COLORS.returnFlare, COLORS.returnGlow, Math.max(0.25, 0.45 * targetProjs[i].scale));
          }
        } else {
          if (jumpProjs[jumpIdx].visible && hostProj.visible) {
            drawFlare(jumpProjs[jumpIdx], hostProj, (eased - 0.5) * 2, COLORS.returnFlare, COLORS.returnGlow, Math.max(0.25, 0.45 * jumpProjs[jumpIdx].scale));
          }
        }
      });
    }

    // Draw nodes back-to-front for depth sorting.
    const allNodes = [
      { proj: hostProj, node: nodes.host },
      ...jumpProjs.map((p, i) => ({ proj: p, node: nodes.jumps[i] })),
      ...targetProjs.map((p, i) => ({ proj: p, node: nodes.targets[i] })),
    ].sort((a, b) => b.proj.z - a.proj.z);

    allNodes.forEach(({ proj, node }) => {
      if (!proj.visible) return;
      if (node.type === 'host') {
        drawHostRing(proj, node.r, (cycleT / CYCLE) * Math.PI * 3);
        drawNode(proj, COLORS.host, COLORS.hostGlow, 14, node.r);
      } else if (node.type === 'jump') {
        drawNode(proj, COLORS.jump, COLORS.jumpGlow, 6, node.r);
      } else {
        drawNode(proj, COLORS.target, null, 0, node.r);
      }
    });

    requestAnimationFrame(render);
  }

  window.addEventListener('resize', resize);
  resize();
  requestAnimationFrame(render);
})();
