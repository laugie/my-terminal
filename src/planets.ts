// 拓扑 3D 星球视图的布局。纯函数，不依赖 three.js 和 DOM，渲染在 planet-view.ts。
// 每个区域一条倾斜的圆形轨道，区域里的设备等距排在轨道上，中心是一颗恒星。

import { deviceKind, type DeviceKind } from "./diagram.js";
import type { Presence, TopologyView } from "./topology.js";

export interface Vec3 {
  x: number;
  y: number;
  z: number;
}

export interface Ring {
  region: string;
  radius: number;
  /** Inclination about the x axis, radians. */
  tilt: number;
  /** Turn about the y axis after the tilt, radians. */
  yaw: number;
  color: string;
}

export interface Planet {
  name: string;
  region: string;
  kind: DeviceKind;
  host: string;
  port: number;
  mobile: boolean;
  presence: Presence;
  position: Vec3;
  radius: number;
  color: string;
  /** Stable per name, for the surface pattern. */
  seed: number;
}

export interface PlanetLink {
  from: string;
  to: string;
  crossRegion: boolean;
}

export interface PlanetModel {
  rings: Ring[];
  planets: Planet[];
  links: PlanetLink[];
  /** Radius of a sphere around the origin that holds every planet. */
  extent: number;
}

export const KIND_COLORS: Readonly<Record<DeviceKind, string>> = {
  server: "#6c8cff",
  cloud: "#4fc3f7",
  nas: "#b388ff",
  router: "#ffb454",
  mac: "#d7dde5",
  windows: "#ff7aa2",
  generic: "#8fa3b8",
};

/**
 * Planet radius by device type: cloud hosts are logged into most, routers
 * are the usual jump hosts, mobile devices are rarely used.
 */
const KIND_SIZES: Readonly<Record<DeviceKind, number>> = {
  cloud: 3,
  router: 2.4,
  server: 2.1,
  nas: 2,
  mac: 1.8,
  windows: 1.8,
  generic: 1.75,
};
const MOBILE_SIZE = 1.25;

const RING_COLORS = ["#79a8ff", "#56c2d6", "#c99cf0", "#e6c07b", "#8fd18a", "#f2777a"];

/** Innermost orbit, gap between orbits, and arc length per planet on an orbit. */
const FIRST_RADIUS = 20;
const RING_GAP = 15;
const SPACING = 14;
const MAX_TILT = 0.45;
/** Phases tried per orbit when staggering it against the inner ones. */
const PHASE_STEPS = 24;

export function buildPlanets(view: TopologyView): PlanetModel {
  const rings: Ring[] = [];
  const planets: Planet[] = [];
  let previous = 0;
  for (const region of view.regions) {
    const count = region.devices.length;
    if (count === 0) {
      continue;
    }
    const index = rings.length;
    const needed = (count * SPACING) / (2 * Math.PI);
    const radius = Math.max(index === 0 ? FIRST_RADIUS : previous + RING_GAP, needed);
    previous = radius;
    const sign = index % 2 === 0 ? 1 : -1;
    const ring: Ring = {
      region: region.name,
      radius,
      tilt: sign * Math.min(0.1 + 0.07 * index, MAX_TILT),
      yaw: index * 0.9,
      color: RING_COLORS[index % RING_COLORS.length] ?? "#79a8ff",
    };
    rings.push(ring);
    const phase = staggerPhase(ring, count, planets);
    region.devices.forEach((device, deviceIndex) => {
      const kind = deviceKind({ name: device.name, type: device.type });
      planets.push({
        name: device.name,
        region: region.name,
        kind,
        host: device.host,
        port: device.port,
        mobile: device.mobile,
        presence: device.presence,
        position: ringPoint(ring, phase + (2 * Math.PI * deviceIndex) / count),
        radius: device.mobile ? MOBILE_SIZE : KIND_SIZES[kind],
        color: KIND_COLORS[kind],
        seed: hashSeed(device.name),
      });
    });
  }

  const byName = new Map(planets.map((planet) => [planet.name, planet]));
  const links: PlanetLink[] = [];
  for (const planet of planets) {
    const device = view.regions
      .find((region) => region.name === planet.region)
      ?.devices.find((item) => item.name === planet.name);
    const jump = typeof device?.jump === "string" ? device.jump.trim() : "";
    const from = jump ? byName.get(jump) : undefined;
    if (!from) {
      continue;
    }
    links.push({ from: from.name, to: planet.name, crossRegion: from.region !== planet.region });
  }

  const outer = rings.reduce((max, ring) => Math.max(max, ring.radius), 0);
  return { rings, planets, links, extent: outer + 2 * Math.max(...Object.values(KIND_SIZES)) };
}

/**
 * Turn an orbit so its planets sit as far as possible from those already
 * placed, instead of lining up with them along a radius.
 */
function staggerPhase(ring: Ring, count: number, placed: readonly Planet[]): number {
  if (placed.length === 0) {
    return 0;
  }
  const step = (2 * Math.PI) / count;
  let best = 0;
  let bestDistance = -1;
  for (let i = 0; i < PHASE_STEPS; i += 1) {
    const phase = (step * i) / PHASE_STEPS;
    let nearest = Infinity;
    for (let k = 0; k < count; k += 1) {
      const point = ringPoint(ring, phase + step * k);
      for (const other of placed) {
        nearest = Math.min(
          nearest,
          Math.hypot(point.x - other.position.x, point.y - other.position.y, point.z - other.position.z),
        );
      }
    }
    if (nearest > bestDistance + 1e-9) {
      best = phase;
      bestDistance = nearest;
    }
  }
  return best;
}

/** Point at `angle` on a tilted, turned circle around the origin. */
export function ringPoint(ring: Ring, angle: number): Vec3 {
  const x = ring.radius * Math.cos(angle);
  const z0 = ring.radius * Math.sin(angle);
  const y = -z0 * Math.sin(ring.tilt);
  const z = z0 * Math.cos(ring.tilt);
  return {
    x: x * Math.cos(ring.yaw) + z * Math.sin(ring.yaw),
    y,
    z: -x * Math.sin(ring.yaw) + z * Math.cos(ring.yaw),
  };
}

/** Quadratic arc from `a` to `b`, raised above the straight line. */
export function linkCurve(a: Vec3, b: Vec3, segments = 24): Vec3[] {
  const length = Math.hypot(b.x - a.x, b.y - a.y, b.z - a.z);
  const control = {
    x: (a.x + b.x) / 2,
    y: (a.y + b.y) / 2 + length * 0.35 + 1.5,
    z: (a.z + b.z) / 2,
  };
  const points: Vec3[] = [{ ...a }];
  for (let i = 1; i < segments; i += 1) {
    const t = i / segments;
    const u = 1 - t;
    points.push({
      x: u * u * a.x + 2 * u * t * control.x + t * t * b.x,
      y: u * u * a.y + 2 * u * t * control.y + t * t * b.y,
      z: u * u * a.z + 2 * u * t * control.z + t * t * b.z,
    });
  }
  points.push({ ...b });
  return points;
}

/** Same colors as the 2D diagram. `none` has no indicator. */
export function presenceColor(presence: Presence): string | null {
  switch (presence) {
    case "online":
      return "#3fb950";
    case "offline":
      return "#6e7a8a";
    case "unknown":
      return "#d6a64a";
    case "none":
      return null;
  }
}

/** Room kept around each planet for its name label, in world units, at label scale 1. */
const LABEL_ROOM = 3;
/** Share of the half-height and half-width the system may fill. */
const FILL = 0.95;
/** Camera heights tried, in degrees above the orbits: lower ones flatten the orbits for wide windows. */
const LOWEST = 22;
const HIGHEST = 37;
/** A higher camera separates near and far planets, so it wins unless a lower one is this much closer. */
const HEIGHT_PREFERENCE = 1.03;
/** Turns checked so the system stays in view while the camera rotates. */
const AZIMUTHS = 8;
const ORBIT_SAMPLES = 24;

export interface CameraFrame {
  /** From the target toward the camera. */
  direction: Vec3;
  distance: number;
  /** Point the camera looks at and turns about; always on the vertical axis through the sun. */
  target: Vec3;
}

/**
 * Camera that shows the whole system as large as the window allows: the
 * height above the orbits follows the window shape, the target moves up or
 * down the sun's axis to centre the system, and every turn about that axis
 * keeps all orbits, planets and labels in view.
 */
export function frameCamera(
  model: PlanetModel,
  fovDegrees: number,
  aspect: number,
  options: { azimuth?: number; labelScale?: number } = {},
): CameraFrame {
  const azimuth = options.azimuth ?? 0;
  const tanV = Math.tan((fovDegrees * Math.PI) / 360) * FILL;
  const tanH = tanV * aspect;
  const points = framePoints(model, LABEL_ROOM * (options.labelScale ?? 1));
  const reach = Math.max(1, model.extent);

  let best: { elevation: number; distance: number; targetY: number } | null = null;
  const fits: Array<{ elevation: number; distance: number; targetY: number }> = [];
  for (let degrees = LOWEST; degrees <= HIGHEST; degrees += 1) {
    const elevation = (degrees * Math.PI) / 180;
    const bases = Array.from({ length: AZIMUTHS }, (_, i) =>
      cameraBasis(orbitDirection(azimuth + (2 * Math.PI * i) / AZIMUTHS, elevation)),
    );
    const distanceAt = (targetY: number) => fitDistance(points, bases, targetY, tanH, tanV);
    // Coarse scan, then a finer one around the best target height.
    let targetY = 0;
    let distance = distanceAt(0);
    let step = reach / 16;
    let centre = 0;
    for (let round = 0; round < 3; round += 1) {
      for (let k = -8; k <= 8; k += 1) {
        const y = centre + k * step;
        const d = distanceAt(y);
        if (d < distance - 1e-9) {
          distance = d;
          targetY = y;
        }
      }
      centre = targetY;
      step /= 8;
    }
    const fit = { elevation, distance, targetY };
    fits.push(fit);
    if (!best || distance < best.distance) {
      best = fit;
    }
  }
  const chosen = best
    ? fits.reduce((pick, fit) =>
        fit.distance <= best.distance * HEIGHT_PREFERENCE && fit.elevation > pick.elevation ? fit : pick,
      )
    : { elevation: (HIGHEST * Math.PI) / 180, distance: 8, targetY: 0 };
  return {
    direction: orbitDirection(azimuth, chosen.elevation),
    distance: chosen.distance,
    target: { x: 0, y: chosen.targetY, z: 0 },
  };
}

/** Orbit samples and planets, each with the room it needs around it. */
function framePoints(model: PlanetModel, labelRoom: number): Array<{ point: Vec3; pad: number }> {
  const points: Array<{ point: Vec3; pad: number }> = [];
  for (const ring of model.rings) {
    for (let i = 0; i < ORBIT_SAMPLES; i += 1) {
      points.push({ point: ringPoint(ring, (i / ORBIT_SAMPLES) * Math.PI * 2), pad: 0 });
    }
  }
  for (const planet of model.planets) {
    points.push({ point: planet.position, pad: planet.radius + labelRoom });
  }
  return points;
}

/** Smallest distance from the target at which every point is in view from every basis. */
function fitDistance(
  points: ReadonlyArray<{ point: Vec3; pad: number }>,
  bases: ReadonlyArray<{ right: Vec3; up: Vec3; forward: Vec3 }>,
  targetY: number,
  tanH: number,
  tanV: number,
): number {
  let distance = 8;
  for (const basis of bases) {
    for (const { point, pad } of points) {
      const p = { x: point.x, y: point.y - targetY, z: point.z };
      const x = Math.abs(dot(p, basis.right)) + pad;
      const y = Math.abs(dot(p, basis.up)) + pad;
      const z = dot(p, basis.forward);
      distance = Math.max(distance, z + x / tanH, z + y / tanV);
    }
  }
  return distance;
}

/** Unit vector toward a camera turned `azimuth` about the vertical axis, `elevation` above the orbits. */
function orbitDirection(azimuth: number, elevation: number): Vec3 {
  return {
    x: Math.sin(azimuth) * Math.cos(elevation),
    y: Math.sin(elevation),
    z: Math.cos(azimuth) * Math.cos(elevation),
  };
}

/** Label size for a stage in CSS pixels: 1 up to a laptop screen, 1.15 on a 27-inch display. */
export function labelScale(width: number, height: number): number {
  const size = Math.min(width, (height * 16) / 9);
  const t = Math.min(Math.max((size - 1400) / (2300 - 1400), 0), 1);
  return 1 + 0.15 * t;
}

/** Right, up and toward-camera unit vectors for a camera on `direction`. */
export function cameraBasis(direction: Vec3): { right: Vec3; up: Vec3; forward: Vec3 } {
  const forward = normalize(direction);
  const right = normalize(cross({ x: 0, y: 1, z: 0 }, forward));
  const up = cross(forward, right);
  return { right, up, forward };
}

function dot(a: Vec3, b: Vec3): number {
  return a.x * b.x + a.y * b.y + a.z * b.z;
}

function cross(a: Vec3, b: Vec3): Vec3 {
  return { x: a.y * b.z - a.z * b.y, y: a.z * b.x - a.x * b.z, z: a.x * b.y - a.y * b.x };
}

function normalize(a: Vec3): Vec3 {
  const length = Math.hypot(a.x, a.y, a.z) || 1;
  return { x: a.x / length, y: a.y / length, z: a.z / length };
}

/** FNV-1a, so a device keeps its surface pattern between reloads. */
export function hashSeed(text: string): number {
  let hash = 0x811c9dc5;
  for (let i = 0; i < text.length; i += 1) {
    hash ^= text.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return hash >>> 0;
}
