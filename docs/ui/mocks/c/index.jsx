import { useState } from "react";

const g = (n) => n * 4;

const screens = [
  { id: "chat", name: "Chat", desc: "Conversation view, empty state with flavor wash, message bubbles, input bar. Orb glows.", file: "c/chat.jsx" },
  { id: "settings", name: "Settings", desc: "Hub (orb glow + wash behind profile) + 7 sub-pages: Appearance, Models, Sound, Trust, Phone, Keys, About", file: "c/settings.jsx" },
  { id: "overlay", name: "Overlay", desc: "Search Bar (replaces Pixel Google bar — orb + status + mic), slide-up Panel, Dynamic Island — all states, all theme-aware", file: "c/overlay.jsx" },
  { id: "onboarding", name: "Onboarding", desc: "9 steps: Welcome (hero orb glow + wash), Appearance (live flavor/theme picker), Conversation Style, Getting to Know You, API Key, Permissions, Trust Level, Choose Your Plan (paywall), Done (wash)", file: "c/onboarding.jsx" },
];

const rules = [
  { label: "Spatial Grid", value: "4pt base, 8pt standard increment" },
  { label: "Touch Targets", value: "44pt minimum (Apple HIG)" },
  { label: "Typography", value: "SF Pro metrics, -0.2 to -0.8 tracking" },
  { label: "Dark Surfaces", value: "#000 → #1C1C1E → #2C2C2E → #3A3A3C → #48484A" },
  { label: "Light Surfaces", value: "#FFF → #F2F2F7 → #E5E5EA → #D1D1D6 → #C7C7CC" },
  { label: "Flavor Strategy", value: "Three touch-points: orb, user bubbles, send button" },
  { label: "No Flavor", value: "White orb (dark) / black orb (light), neutral surfaces" },
  { label: "No Flavor Bubbles", value: "#48484A (dark) / #C7C7CC (light) — one step above assistant" },
  { label: "Icons", value: "SVG, 20×20 nav / 14×14 inline, 1.5px stroke, round cap+join" },
  { label: "Animations", value: "State transitions only, no infinite loops on daily-use screens" },
  { label: "★ Orb Glow", value: "box-shadow: 0 0 <45%>px <18%>px rgba(flavor, 0.15) — applied to every orb instance" },
  { label: "★ Empty Wash", value: "radial-gradient at 3% flavor opacity behind orb on empty/welcome/done screens. No wash for 'none' flavor." },
];

const glowTokens = [
  { flavor: "None", glow: "rgba(255,255,255,0.06) / rgba(0,0,0,0.04)", wash: "— (no wash)" },
  { flavor: "Lemon", glow: "rgba(255,214,0,0.15)", wash: "rgba(255,214,0,0.03)" },
  { flavor: "Tangerine", glow: "rgba(255,140,0,0.15)", wash: "rgba(255,140,0,0.03)" },
  { flavor: "Lime", glow: "rgba(124,179,66,0.15)", wash: "rgba(124,179,66,0.03)" },
  { flavor: "Blood Orange", glow: "rgba(216,67,21,0.15)", wash: "rgba(216,67,21,0.03)" },
  { flavor: "Grapefruit", glow: "rgba(233,30,99,0.15)", wash: "rgba(233,30,99,0.03)" },
];

export default function MockIndex() {
  return (
    <div style={{
      fontFamily: "-apple-system, 'SF Pro Text', system-ui, sans-serif",
      background: "#111", minHeight: "100vh", padding: `${g(10)}px ${g(6)}px`,
      WebkitFontSmoothing: "antialiased", color: "#fff",
      maxWidth: 640, margin: "0 auto",
    }}>
      <div style={{ display: "inline-block", padding: `${g(1)}px ${g(3)}px`, borderRadius: g(1.5), background: "rgba(255,140,0,0.12)", color: "#FF8C00", fontSize: 12, fontWeight: 600, letterSpacing: 0.5, marginBottom: g(3) }}>DIRECTIVE C</div>

      <h1 style={{ fontSize: 28, fontWeight: 800, letterSpacing: -0.8, margin: 0 }}>
        Fawx UI Mockups
      </h1>
      <p style={{ fontSize: 15, color: "rgba(235,235,245,0.60)", letterSpacing: -0.2, margin: `${g(1.5)}px 0 ${g(3)}px`, lineHeight: "22px" }}>
        iOS-Minimal + Orb Presence. The current three-touch-point approach with two additions: orb glow via box-shadow, and a 3% flavor wash on empty/welcome states.
      </p>
      <p style={{ fontSize: 13, color: "rgba(235,235,245,0.30)", letterSpacing: -0.1, margin: `0 0 ${g(8)}px`, lineHeight: "18px" }}>
        Every screen supports dark/light theme, all 5 flavors, and "No Flavor" monochrome mode.
      </p>

      {/* Screen cards */}
      <div style={{ display: "flex", flexDirection: "column", gap: g(3), marginBottom: g(10) }}>
        {screens.map(s => (
          <div key={s.id} style={{
            padding: `${g(4)}px ${g(5)}px`, borderRadius: g(3),
            background: "#1C1C1E", border: "1px solid rgba(84,84,88,0.36)",
          }}>
            <div style={{ display: "flex", alignItems: "baseline", gap: g(3), marginBottom: g(1.5) }}>
              <div style={{ fontSize: 17, fontWeight: 600, letterSpacing: -0.3 }}>{s.name}</div>
              <code style={{ fontSize: 12, color: "rgba(235,235,245,0.30)", fontFamily: "SF Mono, monospace" }}>{s.file}</code>
            </div>
            <div style={{ fontSize: 14, color: "rgba(235,235,245,0.60)", lineHeight: "20px", letterSpacing: -0.15 }}>
              {s.desc}
            </div>
          </div>
        ))}
      </div>

      {/* Design rules reference */}
      <h2 style={{ fontSize: 20, fontWeight: 700, letterSpacing: -0.5, margin: `0 0 ${g(4)}px` }}>Design Rules</h2>
      <div style={{ background: "#1C1C1E", borderRadius: g(3), overflow: "hidden", border: "1px solid rgba(84,84,88,0.36)" }}>
        {rules.map((r, i) => (
          <div key={i} style={{
            display: "flex", padding: `${g(3)}px ${g(5)}px`,
            borderBottom: i < rules.length - 1 ? "0.5px solid rgba(84,84,88,0.20)" : "none",
            gap: g(4),
            background: r.label.startsWith("★") ? "rgba(255,140,0,0.04)" : "transparent",
          }}>
            <div style={{ width: 140, flexShrink: 0, fontSize: 13, fontWeight: 600, color: r.label.startsWith("★") ? "#FF8C00" : "rgba(235,235,245,0.60)", letterSpacing: -0.1 }}>{r.label}</div>
            <div style={{ fontSize: 13, color: "#fff", letterSpacing: -0.1, lineHeight: "18px", fontFamily: r.value.startsWith("#") || r.value.startsWith("box") || r.value.startsWith("radial") ? "SF Mono, monospace" : "inherit" }}>{r.value}</div>
          </div>
        ))}
      </div>

      {/* Directive C specific tokens */}
      <h2 style={{ fontSize: 20, fontWeight: 700, letterSpacing: -0.5, margin: `${g(10)}px 0 ${g(2)}px` }}>Glow & Wash Tokens</h2>
      <p style={{ fontSize: 13, color: "rgba(235,235,245,0.30)", margin: `0 0 ${g(4)}px`, lineHeight: "18px" }}>
        These are the two new token types added in Directive C.
      </p>
      <div style={{ background: "#1C1C1E", borderRadius: g(3), overflow: "hidden", border: "1px solid rgba(84,84,88,0.36)" }}>
        {/* Header */}
        <div style={{ display: "flex", padding: `${g(2.5)}px ${g(5)}px`, borderBottom: "0.5px solid rgba(84,84,88,0.20)", gap: g(4) }}>
          <div style={{ width: 100, flexShrink: 0, fontSize: 11, fontWeight: 600, color: "rgba(235,235,245,0.30)", letterSpacing: 0.5, textTransform: "uppercase" }}>Flavor</div>
          <div style={{ flex: 1, fontSize: 11, fontWeight: 600, color: "rgba(235,235,245,0.30)", letterSpacing: 0.5, textTransform: "uppercase" }}>Glow (box-shadow)</div>
          <div style={{ flex: 1, fontSize: 11, fontWeight: 600, color: "rgba(235,235,245,0.30)", letterSpacing: 0.5, textTransform: "uppercase" }}>Wash (radial-gradient)</div>
        </div>
        {glowTokens.map((row, i) => (
          <div key={i} style={{ display: "flex", padding: `${g(2.5)}px ${g(5)}px`, borderBottom: i < glowTokens.length - 1 ? "0.5px solid rgba(84,84,88,0.20)" : "none", gap: g(4), alignItems: "center" }}>
            <div style={{ width: 100, flexShrink: 0, display: "flex", alignItems: "center", gap: g(2) }}>
              <div style={{
                width: g(4), height: g(4), borderRadius: g(2),
                background: row.flavor === "None"
                  ? "linear-gradient(135deg, #fff 50%, #000 50%)"
                  : { Lemon: "#FFD600", Tangerine: "#FF8C00", Lime: "#7CB342", "Blood Orange": "#D84315", Grapefruit: "#E91E63" }[row.flavor],
              }} />
              <span style={{ fontSize: 13, fontWeight: 500, color: "#fff" }}>{row.flavor}</span>
            </div>
            <div style={{ flex: 1, fontSize: 11, color: "rgba(235,235,245,0.60)", fontFamily: "SF Mono, monospace", letterSpacing: -0.3 }}>{row.glow}</div>
            <div style={{ flex: 1, fontSize: 11, color: "rgba(235,235,245,0.60)", fontFamily: "SF Mono, monospace", letterSpacing: -0.3 }}>{row.wash}</div>
          </div>
        ))}
      </div>

      {/* Color swatches */}
      <h2 style={{ fontSize: 20, fontWeight: 700, letterSpacing: -0.5, margin: `${g(10)}px 0 ${g(4)}px` }}>Flavor Palette</h2>
      <div style={{ display: "flex", gap: g(3), marginBottom: g(4) }}>
        <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: g(2) }}>
          <div style={{ width: g(12), height: g(12), borderRadius: g(6), background: "linear-gradient(135deg, #fff 50%, #000 50%)", border: "1px solid rgba(84,84,88,0.36)" }} />
          <span style={{ fontSize: 11, color: "rgba(235,235,245,0.30)" }}>None</span>
        </div>
        {[
          ["Lemon", "#FFD600"],
          ["Tangerine", "#FF8C00"],
          ["Lime", "#7CB342"],
          ["Blood Orange", "#D84315"],
          ["Grapefruit", "#E91E63"],
        ].map(([name, color]) => (
          <div key={name} style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: g(2) }}>
            <div style={{ width: g(12), height: g(12), borderRadius: g(6), background: color }} />
            <span style={{ fontSize: 11, color: "rgba(235,235,245,0.30)" }}>{name}</span>
          </div>
        ))}
      </div>

      {/* What changed from base */}
      <h2 style={{ fontSize: 20, fontWeight: 700, letterSpacing: -0.5, margin: `${g(10)}px 0 ${g(4)}px` }}>Delta from iOS-Minimal Base</h2>
      <div style={{ background: "#1C1C1E", borderRadius: g(3), overflow: "hidden", border: "1px solid rgba(84,84,88,0.36)", padding: `${g(4)}px ${g(5)}px` }}>
        <div style={{ fontSize: 14, color: "rgba(235,235,245,0.60)", lineHeight: "22px", letterSpacing: -0.15 }}>
          <p style={{ margin: `0 0 ${g(3)}px` }}>Directive C adds exactly <strong style={{ color: "#fff" }}>two CSS properties</strong> to the base iOS-Minimal approach:</p>
          <p style={{ margin: `0 0 ${g(2)}px` }}><strong style={{ color: "#FF8C00" }}>1. Orb glow</strong> — <code style={{ fontSize: 12, fontFamily: "SF Mono, monospace", color: "rgba(235,235,245,0.30)" }}>box-shadow</code> on every <code style={{ fontSize: 12, fontFamily: "SF Mono, monospace", color: "rgba(235,235,245,0.30)" }}>&lt;Orb /&gt;</code> instance. Shadow radius scales proportionally with orb size (45% of diameter). Cost: ~0 GPU — a single shadow pass composited by the system.</p>
          <p style={{ margin: `0 0 ${g(2)}px` }}><strong style={{ color: "#FF8C00" }}>2. Empty-state wash</strong> — <code style={{ fontSize: 12, fontFamily: "SF Mono, monospace", color: "rgba(235,235,245,0.30)" }}>radial-gradient</code> at 3% flavor opacity behind the orb on hero/empty screens only (chat empty, welcome, done). Disappears when a conversation starts. Cost: a single gradient fill, no compositing.</p>
          <p style={{ margin: `0 0 ${g(2)}px` }}>Everything else is identical: opaque stepped surfaces, three-touch-point flavor strategy, "No Flavor" support, 4pt grid, SF Pro typography, SVG icons.</p>
          <p style={{ margin: 0, color: "rgba(235,235,245,0.30)" }}>No new runtime dependencies. No blur. No palette generation. Implementable in Compose with <code style={{ fontSize: 12, fontFamily: "SF Mono, monospace" }}>Modifier.shadow()</code> and a single <code style={{ fontSize: 12, fontFamily: "SF Mono, monospace" }}>Brush.radialGradient()</code>.</p>
        </div>
      </div>
    </div>
  );
}