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

const KIND_SIZES: Readonly<Record<DeviceKind, number>> = {
  server: 1.75,
  cloud: 1.95,
  nas: 1.6,
  router: 1.35,
  mac: 1.45,
  windows: 1.45,
  generic: 1.4,
};

const RING_COLORS = ["#79a8ff", "#56c2d6", "#c99cf0", "#e6c07b", "#8fd18a", "#f2777a"];

/** Innermost orbit, gap between orbits, and arc length per planet on an orbit. */
const FIRST_RADIUS = 10;
const RING_GAP = 8;
const SPACING = 7;
const MAX_TILT = 0.45;

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
    const phase = index * 0.7;
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
        radius: KIND_SIZES[kind],
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

/** Room kept around each planet for its name label, in world units. */
const LABEL_ROOM = 3;

/**
 * Distance from the origin, looking from `direction`, at which every orbit
 * and planet (with its label) is inside the view, with a small margin.
 */
export function cameraDistance(
  model: PlanetModel,
  fovDegrees: number,
  aspect: number,
  direction: Vec3,
): number {
  const tanV = Math.tan((fovDegrees * Math.PI) / 360) * 0.9;
  const tanH = tanV * aspect;
  const basis = cameraBasis(direction);
  const points: Array<{ point: Vec3; pad: number }> = [];
  for (const ring of model.rings) {
    for (let i = 0; i < 36; i += 1) {
      points.push({ point: ringPoint(ring, (i / 36) * Math.PI * 2), pad: 0 });
    }
  }
  for (const planet of model.planets) {
    points.push({ point: planet.position, pad: planet.radius + LABEL_ROOM });
  }
  let distance = 8;
  for (const { point, pad } of points) {
    const x = Math.abs(dot(point, basis.right)) + pad;
    const y = Math.abs(dot(point, basis.up)) + pad;
    const z = dot(point, basis.forward);
    distance = Math.max(distance, z + x / tanH, z + y / tanV);
  }
  return distance;
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
