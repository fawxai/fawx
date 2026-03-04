import { useState } from "react";

const g = (n) => n * 4;

const screens = [
  { id: "chat", name: "Chat", desc: "Conversation view, empty state, message bubbles, input bar", file: "chat.jsx" },
  { id: "settings", name: "Settings", desc: "Hub + 7 sub-pages: Appearance, Models, Sound, Trust, Phone, Keys, About", file: "settings.jsx" },
  { id: "overlay", name: "Overlay", desc: "Bubble, slide-up panel, Dynamic Island — with idle/executing/completed/failed states", file: "overlay.jsx" },
  { id: "onboarding", name: "Onboarding", desc: "Welcome, API key setup, permissions, trust level, completion", file: "onboarding.jsx" },
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
];

export default function MockIndex() {
  return (
    <div style={{
      fontFamily: "-apple-system, 'SF Pro Text', system-ui, sans-serif",
      background: "#111", minHeight: "100vh", padding: `${g(10)}px ${g(6)}px`,
      WebkitFontSmoothing: "antialiased", color: "#fff",
      maxWidth: 640, margin: "0 auto",
    }}>
      <h1 style={{ fontSize: 28, fontWeight: 800, letterSpacing: -0.8, margin: 0 }}>
        Fawx UI Mockups
      </h1>
      <p style={{ fontSize: 15, color: "rgba(235,235,245,0.60)", letterSpacing: -0.2, margin: `${g(1.5)}px 0 ${g(8)}px`, lineHeight: "22px" }}>
        Every screen in the app, rebuilt with consistent design rules. Each mockup supports dark/light theme switching, all 5 flavor options, and the "No Flavor" monochrome mode.
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
          }}>
            <div style={{ width: 140, flexShrink: 0, fontSize: 13, fontWeight: 600, color: "rgba(235,235,245,0.60)", letterSpacing: -0.1 }}>{r.label}</div>
            <div style={{ fontSize: 13, color: "#fff", letterSpacing: -0.1, lineHeight: "18px", fontFamily: r.value.startsWith("#") ? "SF Mono, monospace" : "inherit" }}>{r.value}</div>
          </div>
        ))}
      </div>

      {/* Color swatches */}
      <h2 style={{ fontSize: 20, fontWeight: 700, letterSpacing: -0.5, margin: `${g(10)}px 0 ${g(4)}px` }}>Flavor Palette</h2>
      <div style={{ display: "flex", gap: g(3), marginBottom: g(4) }}>
        {/* No flavor */}
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
    </div>
  );
}