// #35 前端失败测试：拓扑 3D 星球视图的布局（纯函数，不依赖 three.js 和 DOM）。
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  KIND_COLORS,
  buildPlanets,
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

test("the camera starts far enough back to show every orbit", () => {
  const model = buildPlanets(VIEW);
  for (const aspect of [0.6, 1, 2.2]) {
    const distance = cameraDistance(model.extent, 45, aspect);
    const halfHeight = Math.tan((45 * Math.PI) / 360) * (distance - model.extent);
    assert.ok(halfHeight * Math.min(1, aspect) >= model.extent, `aspect ${aspect}`);
  }
  assert.ok(model.extent >= Math.max(...model.rings.map((ring) => ring.radius)));
});

test("an empty view has no planets", () => {
  const model = buildPlanets({ ...VIEW, regions: [] } as unknown as TopologyView);
  assert.equal(model.planets.length, 0);
  assert.equal(model.rings.length, 0);
});
