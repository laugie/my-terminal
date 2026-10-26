// #35 前端失败测试：拓扑 3D 星球视图的布局（纯函数，不依赖 three.js 和 DOM）。
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  KIND_COLORS,
  buildPlanets,
  cameraBasis,
  cameraDistance,
  linkCurve,
  presenceColor,
  ringPoint,
  type Vec3,
} from "./planets.js";
import type { TopologyView } from "./topology.js";

const dev = (name: string, extra: Record<string, unknown> = {}) => ({
  name, host: "192.0.2.10", port: 22, user: "me", mobile: false, presence: "none" as const, ...extra,
});
const VIEW = {
  configPath: "<config>/topology.json", state: "ok",
  regions: [
    { name: "示例办公室", devices: [dev("office-router", { presence: "online" }), dev("my-macbook", { mobile: true }), dev("打印机")] },
    { name: "空区域", devices: [] },
    { name: "示例云", devices: [dev("bastion", { type: "cloud", host: "jump.example.com" }), dev("app-server", { jump: "bastion", presence: "offline" })] },
    { name: "示例家庭", devices: [dev("home-nas", { jump: "bastion", presence: "unknown" }), dev("ghost", { jump: "missing" })] },
  ],
} as unknown as TopologyView;

const dist = (a: Vec3, b: Vec3) => Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z);
const length = (a: Vec3) => Math.hypot(a.x, a.y, a.z);

test("one orbit per non-empty region, growing outward", () => {
  const model = buildPlanets(VIEW);
  assert.deepEqual(model.rings.map((ring) => ring.region), ["示例办公室", "示例云", "示例家庭"]);
  for (let i = 1; i < model.rings.length; i += 1) {
    assert.ok(model.rings[i]!.radius > model.rings[i - 1]!.radius, "外圈更大");
  }
});

test("every device is one planet on its region's orbit", () => {
  const model = buildPlanets(VIEW);
  assert.deepEqual(
    model.planets.map((planet) => planet.name),
    ["office-router", "my-macbook", "打印机", "bastion", "app-server", "home-nas", "ghost"],
  );
  for (const planet of model.planets) {
    const ring = model.rings.find((item) => item.region === planet.region)!;
    assert.ok(Math.abs(length(planet.position) - ring.radius) < 1e-6, `${planet.name} 在轨道上`);
  }
  assert.equal(model.planets.find((p) => p.name === "bastion")?.kind, "cloud");
  assert.equal(model.planets.find((p) => p.name === "home-nas")?.kind, "nas");
  assert.equal(model.planets.find((p) => p.name === "my-macbook")?.mobile, true);
});

test("planets never overlap, even in a crowded region", () => {
  const crowded = {
    ...VIEW,
    regions: [
      { name: "机房", devices: Array.from({ length: 40 }, (_, i) => dev(`srv-${i}`)) },
      { name: "家", devices: [dev("nas"), dev("mac")] },
    ],
  } as unknown as TopologyView;
  for (const view of [VIEW, crowded]) {
    const { planets } = buildPlanets(view);
    for (let i = 0; i < planets.length; i += 1) {
      for (let j = i + 1; j < planets.length; j += 1) {
        const a = planets[i]!;
        const b = planets[j]!;
        assert.ok(dist(a.position, b.position) > (a.radius + b.radius) * 1.5, `${a.name} / ${b.name}`);
      }
    }
  }
});

test("layout is deterministic", () => {
  assert.deepEqual(buildPlanets(VIEW), buildPlanets(VIEW));
});

test("jump links follow the topology; unknown targets are dropped", () => {
  const links = buildPlanets(VIEW).links.map((link) => `${link.from}->${link.to}:${link.crossRegion}`);
  assert.deepEqual(links.sort(), ["bastion->app-server:false", "bastion->home-nas:true"]);
});

test("a link curve joins the two planets and arcs above the straight line", () => {
  const a = { x: -10, y: 0, z: 0 };
  const b = { x: 10, y: 0, z: 0 };
  const points = linkCurve(a, b, 16);
  assert.equal(points.length, 17);
  assert.deepEqual(points[0], a);
  assert.deepEqual(points[16], b);
  assert.ok(points[8]!.y > 1, "中点抬高");
});

test("ringPoint walks the tilted orbit", () => {
  const ring = { region: "r", radius: 12, tilt: 0.3, yaw: 1.1, color: "#ffffff" };
  for (const angle of [0, 1, 2.5, 4]) {
    assert.ok(Math.abs(length(ringPoint(ring, angle)) - 12) < 1e-9);
  }
});

test("presence keeps the 2D colors; no presence means no indicator", () => {
  assert.equal(presenceColor("online"), "#3fb950");
  assert.equal(presenceColor("offline"), "#6e7a8a");
  assert.equal(presenceColor("unknown"), "#d6a64a");
  assert.equal(presenceColor("none"), null);
  const model = buildPlanets(VIEW);
  assert.equal(model.planets.find((p) => p.name === "office-router")?.presence, "online");
});

test("device kinds have distinct planet colors", () => {
  const colors = Object.values(KIND_COLORS);
  assert.equal(new Set(colors).size, colors.length);
});

test("the camera starts where every orbit and planet is in view, without wasting space", () => {
  const model = buildPlanets(VIEW);
  const direction = { x: 0, y: 0.5, z: 1 };
  const basis = cameraBasis(direction);
  const tanV = Math.tan((45 * Math.PI) / 360);
  for (const aspect of [0.6, 1, 2.2]) {
    const distance = cameraDistance(model, 45, aspect, direction);
    let widest = 0;
    for (const ring of model.rings) {
      for (let i = 0; i < 72; i += 1) {
        const p = ringPoint(ring, (i / 72) * Math.PI * 2);
        const depth = distance - (p.x * basis.forward.x + p.y * basis.forward.y + p.z * basis.forward.z);
        const x = Math.abs(p.x * basis.right.x + p.y * basis.right.y + p.z * basis.right.z) / (depth * tanV * aspect);
        const y = Math.abs(p.x * basis.up.x + p.y * basis.up.y + p.z * basis.up.z) / (depth * tanV);
        assert.ok(x <= 1 && y <= 1, `aspect ${aspect}: 轨道点在画面内`);
        widest = Math.max(widest, x, y);
      }
    }
    assert.ok(widest > 0.55, `aspect ${aspect}: 轨道至少占画面一半 (${widest.toFixed(2)})`);
  }
  assert.ok(model.extent >= Math.max(...model.rings.map((ring) => ring.radius)));
});

test("an empty view has no planets", () => {
  const model = buildPlanets({ ...VIEW, regions: [] } as unknown as TopologyView);
  assert.equal(model.planets.length, 0);
  assert.equal(model.rings.length, 0);
});

// #37：打开时星球挤在中间、名字互相遮挡。轨道拉开、同轨道间隔加大、相邻轨道错开，按类型定大小。

const ORBIT_GAP = 12;

test("#37 orbits start farther out and sit farther apart", () => {
  const model = buildPlanets(VIEW);
  assert.ok(model.rings[0]!.radius >= 16, `第一条轨道 ${model.rings[0]!.radius}`);
  for (let i = 1; i < model.rings.length; i += 1) {
    const gap = model.rings[i]!.radius - model.rings[i - 1]!.radius;
    assert.ok(gap >= ORBIT_GAP, `轨道间距 ${gap}`);
  }
});

test("#37 planets on one orbit are evenly spread and well apart", () => {
  const crowded = {
    ...VIEW,
    regions: [{ name: "机房", devices: Array.from({ length: 24 }, (_, i) => dev(`srv-${i}`)) }, ...VIEW.regions],
  } as unknown as TopologyView;
  for (const view of [VIEW, crowded]) {
    const model = buildPlanets(view);
    for (const ring of model.rings) {
      const members = model.planets.filter((planet) => planet.region === ring.region);
      if (members.length < 2) {
        continue;
      }
      const chord = 2 * ring.radius * Math.sin(Math.PI / members.length);
      for (const planet of members) {
        const nearest = Math.min(
          ...members.filter((other) => other !== planet).map((other) => dist(planet.position, other.position)),
        );
        assert.ok(Math.abs(nearest - chord) < 1e-6, `${planet.name} 等距`);
        assert.ok(nearest >= 10, `${ring.region} 相邻星球间距 ${nearest.toFixed(1)}`);
      }
    }
  }
});

test("#37 neighbouring orbits are staggered, not lined up", () => {
  const model = buildPlanets(VIEW);
  for (const planet of model.planets) {
    const others = model.planets.filter((other) => other.region !== planet.region);
    const nearest = Math.min(...others.map((other) => dist(planet.position, other.position)));
    assert.ok(nearest >= ORBIT_GAP * 1.2, `${planet.name} 与其他轨道最近 ${nearest.toFixed(1)}`);
  }
});

test("#37 planet size follows the device type", () => {
  const sized = {
    ...VIEW,
    regions: [
      {
        name: "全部",
        devices: [
          dev("vps-a", { type: "cloud" }),
          dev("gw", { type: "router" }),
          dev("app", { type: "server" }),
          dev("nas", { type: "nas" }),
          dev("mac", { type: "mac" }),
          dev("win", { type: "windows" }),
          dev("thing"),
          dev("phone", { mobile: true }),
          dev("laptop", { type: "mac", mobile: true }),
        ],
      },
    ],
  } as unknown as TopologyView;
  const size = new Map(buildPlanets(sized).planets.map((planet) => [planet.name, planet.radius]));
  const at = (name: string) => size.get(name)!;
  for (const name of ["gw", "app", "nas", "mac", "win", "thing", "phone", "laptop"]) {
    assert.ok(at("vps-a") > at(name), `云主机大于 ${name}`);
  }
  for (const name of ["app", "nas", "mac", "win", "thing", "phone", "laptop"]) {
    assert.ok(at("gw") > at(name), `路由器大于 ${name}`);
  }
  for (const name of ["app", "nas", "mac", "win", "thing"]) {
    assert.ok(at(name) > at("phone") && at(name) > at("laptop"), `移动设备小于 ${name}`);
  }
});
