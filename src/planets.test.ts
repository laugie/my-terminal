// #35 前端失败测试：拓扑 3D 星球视图的布局（纯函数，不依赖 three.js 和 DOM）。
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  KIND_COLORS,
  buildPlanets,
  cameraBasis,
  frameCamera,
  labelScale,
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
  for (const aspect of [0.6, 1, 2.2]) {
    const frame = frameCamera(model, 45, aspect);
    const fill = coverage(model, frame, 45, aspect);
    assert.ok(fill.inside, `aspect ${aspect}: 轨道点在画面内`);
    assert.ok(Math.max(fill.x, fill.y) > 0.55, `aspect ${aspect}: 轨道至少占画面一半`);
  }
  assert.ok(model.extent >= Math.max(...model.rings.map((ring) => ring.radius)));
});

/**
 * How far the orbits reach toward the screen edges for a camera frame:
 * 1 is the edge. `inside` is false when any orbit point or planet leaves the view.
 */
function coverage(
  model: ReturnType<typeof buildPlanets>,
  frame: ReturnType<typeof frameCamera>,
  fov: number,
  aspect: number,
): { x: number; y: number; inside: boolean } {
  const basis = cameraBasis(frame.direction);
  const tanV = Math.tan((fov * Math.PI) / 360);
  const tanH = tanV * aspect;
  const dot = (a: Vec3, b: Vec3) => a.x * b.x + a.y * b.y + a.z * b.z;
  let x = 0;
  let y = 0;
  let inside = true;
  const check = (point: Vec3, pad: number, orbit: boolean) => {
    const p = { x: point.x - frame.target.x, y: point.y - frame.target.y, z: point.z - frame.target.z };
    const depth = frame.distance - dot(p, basis.forward);
    const sx = (Math.abs(dot(p, basis.right)) + pad) / (depth * tanH);
    const sy = (Math.abs(dot(p, basis.up)) + pad) / (depth * tanV);
    if (sx > 1 + 1e-9 || sy > 1 + 1e-9) {
      inside = false;
    }
    if (orbit) {
      x = Math.max(x, sx);
      y = Math.max(y, sy);
    }
  };
  for (const ring of model.rings) {
    for (let i = 0; i < 72; i += 1) {
      check(ringPoint(ring, (i / 72) * Math.PI * 2), 0, true);
    }
  }
  for (const planet of model.planets) {
    check(planet.position, planet.radius, false);
  }
  return { x, y, inside };
}

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

// #39：27 英寸这类大屏上拓扑挤在中间、两侧大片空白。整体放大约 115%，星球拉得更开，取景随窗口自适应。

const SPREAD_VIEW = {
  ...VIEW,
  regions: [
    { name: "办公室", devices: [dev("gw", { type: "router" }), dev("nas", { type: "nas" }), dev("mac", { type: "mac" }), dev("phone", { mobile: true })] },
    { name: "云", devices: [dev("vps-a", { type: "cloud" }), dev("vps-b", { type: "cloud" }), dev("app"), dev("db")] },
    { name: "家", devices: [dev("home-nas", { type: "nas" }), dev("win", { type: "windows" })] },
  ],
} as unknown as TopologyView;
/** 27-inch display at 100%: 2560 x 1440 minus the toolbar and status line. */
const WIDE = 2560 / 1350;

test("#39 a wide window is filled from side to side", () => {
  const model = buildPlanets(SPREAD_VIEW);
  const fill = coverage(model, frameCamera(model, 45, WIDE), 45, WIDE);
  assert.ok(fill.inside, "大屏上所有轨道和星球都在画面内");
  // #37 left the outer orbit at about 56% of the half-width; 115% of that is 65%.
  assert.ok(fill.x >= 0.8, `外圈轨道横向占到 ${(fill.x * 100).toFixed(0)}%`);
  // Vertically the camera keeps room for the turns where tilted orbits reach higher.
  assert.ok(fill.y >= 0.7, `外圈轨道纵向占到 ${(fill.y * 100).toFixed(0)}%`);
});

test("#39 every window shape keeps everything in view while the camera turns", () => {
  const model = buildPlanets(SPREAD_VIEW);
  for (const aspect of [0.6, 1, 1.4, WIDE, 2.4]) {
    for (const azimuth of [0, 0.4, 1.3, 2.2, 3.5, 5.1]) {
      const frame = frameCamera(model, 45, aspect, { azimuth });
      assert.ok(coverage(model, frame, 45, aspect).inside, `aspect ${aspect} azimuth ${azimuth}`);
    }
  }
});

test("#39 a wider window looks from lower down, within a readable range", () => {
  const model = buildPlanets(SPREAD_VIEW);
  const elevation = (aspect: number) => {
    const { direction } = frameCamera(model, 45, aspect);
    return (Math.asin(direction.y / length(direction)) * 180) / Math.PI;
  };
  for (const aspect of [0.6, 1, 1.4, WIDE, 2.4, 3.2]) {
    const degrees = elevation(aspect);
    assert.ok(degrees >= 20 && degrees <= 38, `aspect ${aspect}: 俯角 ${degrees.toFixed(1)}°`);
  }
  assert.ok(elevation(2.4) < elevation(1), "宽窗口俯角更低");
});

test("#39 the camera turns about the sun's vertical axis", () => {
  const model = buildPlanets(SPREAD_VIEW);
  for (const aspect of [1, WIDE]) {
    const { target } = frameCamera(model, 45, aspect, { azimuth: 1.1 });
    assert.equal(target.x, 0);
    assert.equal(target.z, 0);
  }
});

test("#39 the frame keeps the camera's turn", () => {
  const model = buildPlanets(SPREAD_VIEW);
  const { direction } = frameCamera(model, 45, WIDE, { azimuth: 1.1 });
  assert.ok(Math.abs(Math.atan2(direction.x, direction.z) - 1.1) < 1e-9);
});

test("#39 planets are about 15% larger and spread out further", () => {
  const model = buildPlanets(SPREAD_VIEW);
  const size = new Map(model.planets.map((planet) => [planet.name, planet.radius]));
  assert.ok(size.get("vps-a")! >= 2.6 * 1.15 - 1e-9, `云主机半径 ${size.get("vps-a")}`);
  assert.ok(size.get("phone")! >= 1.05 * 1.15 - 1e-9, `移动设备半径 ${size.get("phone")}`);
  assert.ok(model.rings[0]!.radius >= 20, `第一条轨道 ${model.rings[0]!.radius}`);
  for (let i = 1; i < model.rings.length; i += 1) {
    assert.ok(model.rings[i]!.radius - model.rings[i - 1]!.radius >= 15, "轨道间距");
  }
  // Farther apart relative to their size than in #37 (gap 12, cloud 2.6).
  for (const planet of model.planets) {
    for (const other of model.planets) {
      if (other !== planet) {
        const apart = dist(planet.position, other.position) / (planet.radius + other.radius);
        assert.ok(apart >= 2.2, `${planet.name} / ${other.name} ${apart.toFixed(2)}`);
      }
    }
  }
});

test("#39 labels grow with the window, up to 115%", () => {
  assert.equal(labelScale(1280, 760), 1);
  assert.equal(labelScale(1440, 810) < 1.05, true);
  assert.ok(Math.abs(labelScale(2560, 1350) - 1.15) < 1e-9, "27 英寸全屏 115%");
  assert.ok(Math.abs(labelScale(3840, 2000) - 1.15) < 1e-9, "不超过 115%");
  // A wide but short window is limited by its height.
  assert.ok(labelScale(2560, 700) < 1.05);
  let last = 0;
  for (let width = 800; width <= 3000; width += 100) {
    const scale = labelScale(width, width * 0.56);
    assert.ok(scale >= last, "随窗口变大不减小");
    last = scale;
  }
});
