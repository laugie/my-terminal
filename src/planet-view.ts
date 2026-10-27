// 拓扑 3D 星球视图：每台设备是一颗星球。拖动旋转，滚轮缩放，点击星球或名字连接。
// 由 topology-view.ts 按需动态加载，平面图不会加载 three.js。
// 名字标签是带 .device 和 data-device-name 的按钮，所以键盘操作、当前标签高亮和
// 「最多 8 个标签」提示与平面图共用同一套代码。

import {
  AdditiveBlending,
  AmbientLight,
  BufferGeometry,
  CanvasTexture,
  Color,
  DoubleSide,
  Float32BufferAttribute,
  Group,
  HemisphereLight,
  Line,
  LineBasicMaterial,
  LineDashedMaterial,
  LineLoop,
  Mesh,
  MeshBasicMaterial,
  MeshStandardMaterial,
  PerspectiveCamera,
  PointLight,
  Points,
  PointsMaterial,
  Raycaster,
  RingGeometry,
  Scene,
  SphereGeometry,
  Sprite,
  SpriteMaterial,
  SRGBColorSpace,
  Vector2,
  Vector3,
  WebGLRenderer,
  type Material,
  type Object3D,
  type Texture,
} from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { CSS2DObject, CSS2DRenderer } from "three/examples/jsm/renderers/CSS2DRenderer.js";
import { bindDeviceActivation } from "./diagram.ts";
import { placeLabels, type LabelBox, type LabelSlot } from "./label-layout.ts";
import {
  buildPlanets,
  frameCamera,
  labelScale,
  linkCurve,
  presenceColor,
  ringPoint,
  type Planet,
  type PlanetModel,
} from "./planets.ts";
import type { TopologyView } from "./topology.ts";

const FOV = 45;
const PRESENCE_TEXT = { online: "在线", offline: "离线", unknown: "在线状态未知", none: "" } as const;

export interface PlanetViewOptions {
  /** Same handler as a click in the flat diagram. */
  onActivate(name: string): void;
}

export interface PlanetView {
  readonly element: HTMLElement;
  /** New data rebuilds the planets; the camera stays where the user left it. */
  update(view: TopologyView, options: PlanetViewOptions): void;
  dispose(): void;
}

interface PlanetParts {
  planet: Planet;
  mesh: Mesh;
  halo: Sprite | null;
  selection: Mesh;
  label: HTMLElement;
}

/**
 * A label that the screen layout moves around its anchor. The CSS2D object
 * pins a zero-size anchor to the 3D point; the label is offset inside it.
 */
interface FloatingLabel {
  id: string;
  anchor: Object3D;
  label: HTMLElement;
  /** World radius of what the label belongs to; 0 for an orbit name. */
  radius: number;
  mesh: Mesh | null;
}

export function mountPlanetView(
  host: HTMLElement,
  view: TopologyView,
  initial: PlanetViewOptions,
): PlanetView {
  const stage = document.createElement("div");
  stage.className = "planet-stage";
  host.append(stage);

  const renderer = new WebGLRenderer({ antialias: true, alpha: true });
  renderer.setClearColor(0x000000, 0);
  renderer.outputColorSpace = SRGBColorSpace;
  renderer.domElement.className = "planet-canvas";
  stage.append(renderer.domElement);
  const labels = new CSS2DRenderer();
  labels.domElement.className = "planet-labels";
  stage.append(labels.domElement);

  const hint = document.createElement("p");
  hint.className = "planet-hint";
  hint.textContent = "拖动旋转 · 滚轮缩放 · 点击星球连接 · 双击空白处复位";
  stage.append(hint);

  const scene = new Scene();
  const camera = new PerspectiveCamera(FOV, 1, 0.1, 4000);
  const reducedMotion = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;

  scene.add(new AmbientLight(0xffffff, 0.35));
  scene.add(new HemisphereLight(0x9bb8ff, 0x0b0e13, 0.6));
  const sunLight = new PointLight(0xffe2b0, 2.4, 0, 0);
  scene.add(sunLight);

  const glow = glowTexture();
  const shared: Array<{ dispose(): void }> = [glow];
  scene.add(starfield(shared));
  scene.add(sun(glow, shared));

  const controls = new OrbitControls(camera, renderer.domElement);
  controls.enableDamping = true;
  controls.dampingFactor = 0.08;
  controls.enablePan = false;
  controls.rotateSpeed = 0.6;
  controls.zoomSpeed = 0.9;
  controls.autoRotate = !reducedMotion;
  controls.autoRotateSpeed = 0.35;
  let idleTimer = 0;
  // Until the user turns or zooms, a resize frames the system again.
  let userMoved = false;
  let fitted = 0;
  let scale = 1;
  controls.addEventListener("start", () => {
    userMoved = true;
    controls.autoRotate = false;
    window.clearTimeout(idleTimer);
  });
  controls.addEventListener("end", () => {
    window.clearTimeout(idleTimer);
    if (!reducedMotion) {
      idleTimer = window.setTimeout(() => {
        controls.autoRotate = true;
      }, 20000);
    }
  });

  let options = initial;
  let current: TopologyView | null = null;
  let content = new Group();
  let parts: PlanetParts[] = [];
  let floatingLabels: FloatingLabel[] = [];
  const previousSlots = new Map<string, LabelSlot>();
  let ownedByContent: Array<{ dispose(): void }> = [];
  scene.add(content);
  let latest: PlanetModel | null = null;

  function rebuild(next: TopologyView): void {
    removeContent();
    content = new Group();
    parts = [];
    floatingLabels = [];
    previousSlots.clear();
    ownedByContent = [];
    const model = buildPlanets(next);
    latest = model;
    drawRings(model);
    for (const planet of model.planets) {
      parts.push(drawPlanet(planet));
    }
    drawLinks(model);
    scene.add(content);
    controls.minDistance = 6;
    controls.maxDistance = Math.max(60, model.extent * 6);
    if (!userMoved) {
      frame(model, azimuth());
    }
    // CSS2DRenderer adds a label to the DOM on its first render in view. The
    // camera frames every planet, so one render now puts all labels in the
    // DOM and the caller (and markActive in main.ts) can mark them.
    scene.updateMatrixWorld();
    camera.updateMatrixWorld();
    labels.render(scene, camera);
  }

  /** Removing a parent does not take CSS2D labels out of the DOM; do it here. */
  function removeContent(): void {
    content.traverse((object) => {
      if (object instanceof CSS2DObject) {
        object.element.remove();
      }
    });
    scene.remove(content);
    disposeAll(ownedByContent);
  }

  /** Show the whole system as large as the stage allows, turned `turn` about the sun. */
  function frame(model: PlanetModel, turn = 0): void {
    const fit = frameCamera(model, FOV, camera.aspect || 1, { azimuth: turn, labelScale: scale });
    controls.target.set(fit.target.x, fit.target.y, fit.target.z);
    camera.position
      .set(fit.direction.x, fit.direction.y, fit.direction.z)
      .multiplyScalar(fit.distance)
      .add(controls.target);
    controls.update();
    userMoved = false;
    fitted = fit.distance;
  }

  /** After a resize: frame again, or keep the user's view and zoom relative to the new fit. */
  function refit(): void {
    if (!latest) {
      return;
    }
    if (!userMoved) {
      frame(latest, azimuth());
      return;
    }
    const fit = frameCamera(latest, FOV, camera.aspect, { azimuth: azimuth(), labelScale: scale });
    if (fitted > 0) {
      const offset = camera.position.clone().sub(controls.target).multiplyScalar(fit.distance / fitted);
      camera.position.copy(controls.target).add(offset);
      controls.update();
    }
    fitted = fit.distance;
  }

  /** Current turn of the camera about the vertical axis. */
  function azimuth(): number {
    const x = camera.position.x - controls.target.x;
    const z = camera.position.z - controls.target.z;
    return x === 0 && z === 0 ? 0 : Math.atan2(x, z);
  }

  function drawRings(model: PlanetModel): void {
    for (const ring of model.rings) {
      const points: number[] = [];
      for (let i = 0; i < 128; i += 1) {
        const point = ringPoint(ring, (i / 128) * Math.PI * 2);
        points.push(point.x, point.y, point.z);
      }
      const geometry = new BufferGeometry();
      geometry.setAttribute("position", new Float32BufferAttribute(points, 3));
      const material = new LineBasicMaterial({ color: ring.color, transparent: true, opacity: 0.32 });
      ownedByContent.push(geometry, material);
      content.add(new LineLoop(geometry, material));

      const tag = document.createElement("div");
      tag.className = "planet-region";
      tag.textContent = ring.region;
      tag.style.color = ring.color;
      const anchor = floating(tag);
      const at = ringPoint(ring, Math.PI * 1.5 + 0.35);
      anchor.position.set(at.x, at.y, at.z);
      content.add(anchor);
      floatingLabels.push({ id: `region:${ring.region}`, anchor, label: tag, radius: 0, mesh: null });
    }
  }

  function drawPlanet(planet: Planet): PlanetParts {
    const group = new Group();
    group.position.set(planet.position.x, planet.position.y, planet.position.z);

    const surface = surfaceTexture(planet.color, planet.seed);
    const geometry = new SphereGeometry(planet.radius, 48, 32);
    const offline = planet.presence === "offline";
    const material = new MeshStandardMaterial({
      map: surface,
      color: offline ? 0x8a929c : 0xffffff,
      roughness: 0.78,
      metalness: 0.05,
      emissive: new Color(planet.color),
      emissiveIntensity: offline ? 0 : 0.12,
    });
    ownedByContent.push(surface, geometry, material);
    const mesh = new Mesh(geometry, material);
    mesh.rotation.z = ((planet.seed % 40) - 20) * (Math.PI / 180);
    mesh.userData.deviceName = planet.name;
    group.add(mesh);

    if (planet.kind === "cloud" || planet.kind === "nas") {
      const ringGeometry = new RingGeometry(planet.radius * 1.45, planet.radius * 2.05, 72);
      const ringMaterial = new MeshBasicMaterial({
        color: planet.color,
        side: DoubleSide,
        transparent: true,
        opacity: offline ? 0.18 : 0.38,
      });
      ownedByContent.push(ringGeometry, ringMaterial);
      const ring = new Mesh(ringGeometry, ringMaterial);
      ring.rotation.x = Math.PI / 2.4;
      ring.rotation.y = 0.3;
      group.add(ring);
    }

    let halo: Sprite | null = null;
    const presence = presenceColor(planet.presence);
    if (presence) {
      const haloMaterial = new SpriteMaterial({
        map: glow,
        color: presence,
        transparent: true,
        depthWrite: false,
        blending: AdditiveBlending,
        opacity: planet.presence === "online" ? 0.85 : 0.5,
      });
      ownedByContent.push(haloMaterial);
      halo = new Sprite(haloMaterial);
      halo.scale.setScalar(planet.radius * 4.6);
      group.add(halo);
    }

    const selectionGeometry = new RingGeometry(planet.radius * 1.3, planet.radius * 1.42, 64);
    const selectionMaterial = new MeshBasicMaterial({
      color: 0x8ab4ff,
      side: DoubleSide,
      transparent: true,
      opacity: 0.95,
      depthWrite: false,
    });
    ownedByContent.push(selectionGeometry, selectionMaterial);
    const selection = new Mesh(selectionGeometry, selectionMaterial);
    selection.visible = false;
    group.add(selection);

    const label = planetLabel(planet);
    const anchor = floating(label);
    group.add(anchor);
    floatingLabels.push({ id: `planet:${planet.name}`, anchor, label, radius: planet.radius, mesh });
    bindDeviceActivation(label, planet.name, (name) => {
      options.onActivate(name);
    });
    label.addEventListener("pointerenter", () => {
      hovered = planet.name;
    });
    label.addEventListener("pointerleave", () => {
      if (hovered === planet.name) {
        hovered = null;
      }
    });

    content.add(group);
    return { planet, mesh, halo, selection, label };
  }

  function drawLinks(model: PlanetModel): void {
    const byName = new Map(model.planets.map((planet) => [planet.name, planet]));
    for (const link of model.links) {
      const from = byName.get(link.from);
      const to = byName.get(link.to);
      if (!from || !to) {
        continue;
      }
      const points: number[] = [];
      for (const point of linkCurve(from.position, to.position, 32)) {
        points.push(point.x, point.y, point.z);
      }
      const geometry = new BufferGeometry();
      geometry.setAttribute("position", new Float32BufferAttribute(points, 3));
      const material: Material = link.crossRegion
        ? new LineDashedMaterial({ color: 0x79a8ff, dashSize: 0.9, gapSize: 0.6, transparent: true, opacity: 0.8 })
        : new LineBasicMaterial({ color: 0x79a8ff, transparent: true, opacity: 0.8 });
      ownedByContent.push(geometry, material);
      const line = new Line(geometry, material);
      if (link.crossRegion) {
        line.computeLineDistances();
      }
      content.add(line);
    }
  }

  // Click on a planet: a press that barely moved. A drag only rotates.
  const raycaster = new Raycaster();
  const pointer = new Vector2();
  let pressed: { x: number; y: number; at: number } | null = null;
  let hovered: string | null = null;

  function planetAt(event: MouseEvent): PlanetParts | null {
    const rect = renderer.domElement.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) {
      return null;
    }
    pointer.set(
      ((event.clientX - rect.left) / rect.width) * 2 - 1,
      -((event.clientY - rect.top) / rect.height) * 2 + 1,
    );
    raycaster.setFromCamera(pointer, camera);
    const hit = raycaster.intersectObjects(
      parts.map((part) => part.mesh),
      false,
    )[0];
    const name = hit?.object.userData.deviceName;
    return parts.find((part) => part.planet.name === name) ?? null;
  }

  const canvas = renderer.domElement;
  canvas.addEventListener("pointerdown", (event) => {
    pressed = { x: event.clientX, y: event.clientY, at: performance.now() };
  });
  canvas.addEventListener("pointerup", (event) => {
    const start = pressed;
    pressed = null;
    if (!start || event.button !== 0) {
      return;
    }
    const moved = Math.hypot(event.clientX - start.x, event.clientY - start.y);
    if (moved > 5 || performance.now() - start.at > 600) {
      return;
    }
    const part = planetAt(event);
    if (part) {
      options.onActivate(part.planet.name);
    }
  });
  canvas.addEventListener("pointermove", (event) => {
    if (pressed) {
      return;
    }
    const part = planetAt(event);
    hovered = part?.planet.name ?? null;
    canvas.style.cursor =
      part && !part.label.classList.contains("device-open-disabled") ? "pointer" : "";
  });
  // Double-click on empty space brings the whole system back into view.
  canvas.addEventListener("dblclick", (event) => {
    if (latest && !planetAt(event)) {
      frame(latest);
    }
  });
  canvas.addEventListener("pointerleave", () => {
    hovered = null;
    canvas.style.cursor = "";
  });

  let width = 0;
  let height = 0;
  let pixelRatio = 0;
  function resize(): void {
    const nextWidth = stage.clientWidth;
    const nextHeight = stage.clientHeight;
    const nextRatio = Math.min(window.devicePixelRatio || 1, 2);
    if (nextWidth === width && nextHeight === height && nextRatio === pixelRatio) {
      return;
    }
    width = nextWidth;
    height = nextHeight;
    if (nextRatio !== pixelRatio) {
      pixelRatio = nextRatio;
      renderer.setPixelRatio(pixelRatio);
    }
    if (width === 0 || height === 0) {
      return;
    }
    renderer.setSize(width, height, false);
    labels.setSize(width, height);
    camera.aspect = width / height;
    camera.updateProjectionMatrix();
    scale = labelScale(width, height);
    stage.style.setProperty("--planet-scale", scale.toFixed(3));
    refit();
  }
  const observer = new ResizeObserver(() => {
    resize();
  });
  observer.observe(stage);

  const worldPosition = new Vector3();
  const toCamera = new Vector3();
  let frameHandle = 0;
  let sleepHandle = 0;
  let disposed = false;
  let last = performance.now();

  function tick(now: number): void {
    frameHandle = 0;
    if (disposed) {
      return;
    }
    // Under a terminal tab or the manual list the page is hidden: check again later.
    if (stage.offsetParent === null) {
      sleepHandle = window.setTimeout(() => {
        sleepHandle = 0;
        last = performance.now();
        frameHandle = window.requestAnimationFrame(tick);
      }, 400);
      return;
    }
    const delta = Math.min((now - last) / 1000, 0.1);
    last = now;
    resize();
    controls.update(delta);
    for (const part of parts) {
      const active = part.label.classList.contains("device-active");
      const focused = hovered === part.planet.name || document.activeElement === part.label;
      part.selection.visible = active || focused;
      part.selection.quaternion.copy(camera.quaternion);
      (part.selection.material as MeshBasicMaterial).color.set(active ? 0x8ab4ff : 0xffffff);
      const scale = focused ? 1.12 : 1;
      part.mesh.scale.setScalar(part.mesh.scale.x + (scale - part.mesh.scale.x) * 0.2);
      part.label.classList.toggle("planet-hover", focused);
      if (!reducedMotion) {
        part.mesh.rotation.y += delta * 0.25;
        if (part.halo && part.planet.presence === "online") {
          const pulse = 0.7 + 0.2 * Math.sin(now / 600 + part.planet.seed);
          (part.halo.material as SpriteMaterial).opacity = pulse;
        }
      }
      // Labels on the far side of the sun fade so the near ones stay readable.
      part.mesh.getWorldPosition(worldPosition);
      toCamera.copy(camera.position);
      const behind = worldPosition.distanceTo(toCamera) > toCamera.length();
      part.label.classList.toggle("planet-far", behind);
    }
    layoutLabels();
    renderer.render(scene, camera);
    labels.render(scene, camera);
    frameHandle = window.requestAnimationFrame(tick);
  }

  const projected = new Vector3();
  const anchorPosition = new Vector3();

  /**
   * Place every label on screen so labels do not cover each other or other
   * planets. Reads all sizes first, then writes all offsets.
   */
  function layoutLabels(): void {
    if (width === 0 || height === 0) {
      return;
    }
    const halfHeight = height / 2;
    const focal = halfHeight / Math.tan((FOV * Math.PI) / 360);
    const boxes: LabelBox[] = [];
    const centres = new Map<string, { x: number; y: number }>();
    for (const item of floatingLabels) {
      item.anchor.getWorldPosition(anchorPosition);
      projected.copy(anchorPosition).project(camera);
      if (projected.z < -1 || projected.z > 1) {
        continue;
      }
      const x = ((projected.x + 1) / 2) * width;
      const y = ((1 - projected.y) / 2) * height;
      const depth = anchorPosition.distanceTo(camera.position);
      const scale = item.mesh ? item.mesh.scale.x : 1;
      const r = depth > 0 ? (item.radius * scale * focal) / depth : 0;
      // Orbit names go first so planet labels move around them.
      boxes.push({
        id: item.id,
        x,
        y,
        r,
        w: item.label.offsetWidth,
        h: item.label.offsetHeight,
        depth: item.mesh ? depth : -1,
      });
      centres.set(item.id, { x, y });
    }
    const placed = placeLabels(boxes, { width, height, previous: previousSlots });
    for (const item of floatingLabels) {
      const placement = placed.get(item.id);
      const centre = centres.get(item.id);
      if (!placement || !centre) {
        continue;
      }
      previousSlots.set(item.id, placement.slot);
      const dx = Math.round(placement.left - centre.x);
      const dy = Math.round(placement.top - centre.y);
      item.label.style.transform = `translate(${dx}px, ${dy}px)`;
    }
  }

  resize();
  rebuild(view);
  current = view;
  frameHandle = window.requestAnimationFrame(tick);

  return {
    element: stage,
    update(next, nextOptions) {
      options = nextOptions;
      if (next !== current) {
        current = next;
        rebuild(next);
      }
    },
    dispose() {
      disposed = true;
      window.cancelAnimationFrame(frameHandle);
      window.clearTimeout(sleepHandle);
      window.clearTimeout(idleTimer);
      observer.disconnect();
      controls.dispose();
      removeContent();
      disposeAll(shared);
      renderer.dispose();
      renderer.forceContextLoss();
      stage.remove();
    },
  };
}

/** Zero-size anchor at a 3D point; the label inside is moved by the layout. */
function floating(label: HTMLElement): CSS2DObject {
  const anchor = document.createElement("div");
  anchor.className = "planet-anchor";
  anchor.append(label);
  return new CSS2DObject(anchor);
}

function planetLabel(planet: Planet): HTMLElement {
  const label = document.createElement("div");
  label.className = "device planet-label";
  label.dataset.deviceName = planet.name;
  label.dataset.kind = planet.kind;
  label.tabIndex = 0;
  label.setAttribute("role", "button");
  label.setAttribute("aria-label", `连接 ${planet.name}`);
  const presence = PRESENCE_TEXT[planet.presence];
  label.title = [`${planet.name} ${planet.host}:${planet.port}`, planet.mobile ? "移动设备" : "", presence]
    .filter((part) => part !== "")
    .join(" ");

  const name = document.createElement("span");
  name.className = "planet-name";
  if (planet.presence !== "none") {
    const dot = document.createElement("span");
    dot.className = `planet-presence planet-presence-${planet.presence}`;
    dot.setAttribute("aria-hidden", "true");
    name.append(dot);
  }
  name.append(planet.name);
  if (planet.mobile) {
    const badge = document.createElement("span");
    badge.className = "planet-mobile";
    badge.textContent = "移动";
    name.append(badge);
  }
  const address = document.createElement("span");
  address.className = "planet-address";
  address.textContent = `${planet.host}:${planet.port}`;
  label.append(name, address);
  return label;
}

function sun(glow: Texture, owned: Array<{ dispose(): void }>): Object3D {
  const group = new Group();
  const geometry = new SphereGeometry(2.5, 48, 32);
  const material = new MeshBasicMaterial({ color: 0xffd27a });
  const haloMaterial = new SpriteMaterial({
    map: glow,
    color: 0xffb454,
    transparent: true,
    depthWrite: false,
    blending: AdditiveBlending,
    opacity: 0.9,
  });
  owned.push(geometry, material, haloMaterial);
  group.add(new Mesh(geometry, material));
  const halo = new Sprite(haloMaterial);
  halo.scale.setScalar(18);
  group.add(halo);
  return group;
}

function starfield(owned: Array<{ dispose(): void }>): Object3D {
  const random = seeded(7);
  const positions: number[] = [];
  for (let i = 0; i < 1400; i += 1) {
    const u = random() * 2 - 1;
    const theta = random() * Math.PI * 2;
    const r = 400 + random() * 600;
    const s = Math.sqrt(1 - u * u);
    positions.push(r * s * Math.cos(theta), r * u, r * s * Math.sin(theta));
  }
  const geometry = new BufferGeometry();
  geometry.setAttribute("position", new Float32BufferAttribute(positions, 3));
  const material = new PointsMaterial({
    color: 0xc9d6ff,
    size: 1.6,
    sizeAttenuation: false,
    transparent: true,
    opacity: 0.75,
    depthWrite: false,
  });
  owned.push(geometry, material);
  return new Points(geometry, material);
}

function glowTexture(): CanvasTexture {
  const canvas = document.createElement("canvas");
  canvas.width = 128;
  canvas.height = 128;
  const context = canvas.getContext("2d");
  if (context) {
    const gradient = context.createRadialGradient(64, 64, 0, 64, 64, 64);
    gradient.addColorStop(0, "rgba(255,255,255,1)");
    gradient.addColorStop(0.25, "rgba(255,255,255,0.55)");
    gradient.addColorStop(0.6, "rgba(255,255,255,0.12)");
    gradient.addColorStop(1, "rgba(255,255,255,0)");
    context.fillStyle = gradient;
    context.fillRect(0, 0, 128, 128);
  }
  const texture = new CanvasTexture(canvas);
  texture.colorSpace = SRGBColorSpace;
  return texture;
}

/** Banded surface like a gas giant, varied per device by `seed`. */
function surfaceTexture(base: string, seed: number): CanvasTexture {
  const canvas = document.createElement("canvas");
  canvas.width = 256;
  canvas.height = 128;
  const context = canvas.getContext("2d");
  const random = seeded(seed);
  if (context) {
    const color = new Color(base);
    const hsl = { h: 0, s: 0, l: 0 };
    color.getHSL(hsl);
    let y = 0;
    while (y < canvas.height) {
      const band = 4 + Math.floor(random() * 14);
      const light = Math.min(0.85, Math.max(0.18, hsl.l + (random() - 0.5) * 0.28));
      const hue = (hsl.h + (random() - 0.5) * 0.04 + 1) % 1;
      context.fillStyle = `hsl(${Math.round(hue * 360)}, ${Math.round(hsl.s * 100)}%, ${Math.round(light * 100)}%)`;
      context.fillRect(0, y, canvas.width, band);
      y += band;
    }
    for (let i = 0; i < 90; i += 1) {
      context.fillStyle = `rgba(255,255,255,${(random() * 0.12).toFixed(3)})`;
      const w = 6 + random() * 40;
      context.fillRect(random() * canvas.width, random() * canvas.height, w, 1 + random() * 2);
    }
  }
  const texture = new CanvasTexture(canvas);
  texture.colorSpace = SRGBColorSpace;
  return texture;
}

/** mulberry32: a small deterministic generator for stars and surfaces. */
function seeded(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function disposeAll(items: Array<{ dispose(): void }>): void {
  for (const item of items) {
    item.dispose();
  }
  items.length = 0;
}
