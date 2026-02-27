import { useState } from "react";

const GRID = 4;
const g = (n) => n * GRID;

const themes = {
  dark: {
    bg: "#000000", surface1: "#1C1C1E", surface2: "#2C2C2E", surface3: "#3A3A3C", surface4: "#48484A",
    labelPrimary: "#FFFFFF", labelSecondary: "rgba(235,235,245,0.60)",
    labelTertiary: "rgba(235,235,245,0.30)", labelQuaternary: "rgba(235,235,245,0.18)",
    separator: "rgba(84,84,88,0.36)",
    green: "#30D158", red: "#FF453A",
    noFlavorOrb: "#FFFFFF", noFlavorOrbInner: "rgba(0,0,0,0.12)",
    noFlavorBubble: "#48484A", noFlavorSend: "#3A3A3C", noFlavorSendIcon: "rgba(235,235,245,0.60)",
  },
  light: {
    bg: "#FFFFFF", surface1: "#F2F2F7", surface2: "#E5E5EA", surface3: "#D1D1D6", surface4: "#C7C7CC",
    labelPrimary: "#000000", labelSecondary: "rgba(60,60,67,0.60)",
    labelTertiary: "rgba(60,60,67,0.30)", labelQuaternary: "rgba(60,60,67,0.18)",
    separator: "rgba(60,60,67,0.12)",
    green: "#34C759", red: "#FF3B30",
    noFlavorOrb: "#000000", noFlavorOrbInner: "rgba(255,255,255,0.20)",
    noFlavorBubble: "#C7C7CC", noFlavorSend: "#D1D1D6", noFlavorSendIcon: "rgba(60,60,67,0.60)",
  },
};

const FLAVORS = {
  none:         { primary: null, onPrimary: null, label: "None" },
  lemon:        { primary: "#FFD600", onPrimary: "#1C1A00", label: "Lemon" },
  tangerine:    { primary: "#FF8C00", onPrimary: "#FFFFFF", label: "Tangerine" },
  lime:         { primary: "#7CB342", onPrimary: "#FFFFFF", label: "Lime" },
  blood_orange: { primary: "#D84315", onPrimary: "#FFFFFF", label: "Blood Orange" },
  grapefruit:   { primary: "#E91E63", onPrimary: "#FFFFFF", label: "Grapefruit" },
};

const orbClr = (fk, t) => fk === "none" ? t.noFlavorOrb : FLAVORS[fk].primary;
const orbInn = (fk, t) => fk === "none" ? t.noFlavorOrbInner : "rgba(0,0,0,0.12)";
const sendBg = (fk, t) => fk === "none" ? t.noFlavorSend : FLAVORS[fk].primary;
const sendIc = (fk, t) => fk === "none" ? t.noFlavorSendIcon : FLAVORS[fk].onPrimary;

const Orb = ({ color, inner, size }) => {
  const r = Math.round(size * 0.38);
  return (
    <div style={{ width: size, height: size, borderRadius: size / 2, background: color, display: "grid", placeItems: "center", flexShrink: 0 }}>
      <div style={{ width: r, height: r, borderRadius: r / 2, background: inner }} />
    </div>
  );
};

const SendIcon = (c) => <svg width="14" height="14" viewBox="0 0 14 14" fill="none"><path d="M7 12V2M3 6l4-4 4 4" stroke={c} strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"/></svg>;

/* ── Fake phone home screen backdrop ── */
const PhoneBackdrop = ({ children, t }) => (
  <div style={{
    width: 390, height: 680, borderRadius: g(6), overflow: "hidden", position: "relative",
    background: t.bg === "#000000"
      ? "linear-gradient(160deg, #1a1a2e 0%, #16213e 40%, #0f3460 100%)"
      : "linear-gradient(160deg, #e8ecf4 0%, #d5dce8 40%, #c2cfe0 100%)",
    border: `1px solid ${t.separator}`,
  }}>
    {/* Fake app grid */}
    <div style={{ display: "grid", gridTemplateColumns: "repeat(4, 1fr)", gap: g(5), padding: `${g(16)}px ${g(8)}px`, opacity: 0.15 }}>
      {Array.from({ length: 8 }).map((_, i) => (
        <div key={i} style={{ width: g(14), height: g(14), borderRadius: g(3.5), background: ["#FF3B30","#FF9500","#FFCC00","#34C759","#007AFF","#5856D6","#AF52DE","#FF2D55"][i] }} />
      ))}
    </div>
    {children}
  </div>
);

/* ── Overlay: Bubble ── */
const BubbleOverlay = ({ state, flavor, t }) => (
  <div style={{ position: "absolute", bottom: g(25), right: g(5) }}>
    <div style={{ width: g(13), height: g(13), borderRadius: g(6.5), background: orbClr(flavor, t), display: "grid", placeItems: "center", boxShadow: "0 4px 16px rgba(0,0,0,0.3)" }}>
      <div style={{ width: Math.round(g(13) * 0.38), height: Math.round(g(13) * 0.38), borderRadius: Math.round(g(13) * 0.19), background: orbInn(flavor, t) }} />
    </div>
    {state === "completed" && (
      <div style={{ position: "absolute", top: -2, right: -2, width: g(4), height: g(4), borderRadius: g(2), background: t.green, display: "grid", placeItems: "center", border: `2px solid ${t.bg === "#000000" ? "#1a1a2e" : "#e8ecf4"}` }}>
        <svg width="8" height="8" viewBox="0 0 10 10" fill="none"><path d="M2 5.5L4.2 7.5 8 3" stroke="#fff" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round"/></svg>
      </div>
    )}
    {state === "failed" && (
      <div style={{ position: "absolute", top: -2, right: -2, width: g(4), height: g(4), borderRadius: g(2), background: t.red, display: "grid", placeItems: "center", border: `2px solid ${t.bg === "#000000" ? "#1a1a2e" : "#e8ecf4"}` }}>
        <span style={{ fontSize: 9, fontWeight: 700, color: "#fff" }}>!</span>
      </div>
    )}
    {state === "unread" && (
      <div style={{ position: "absolute", top: -2, right: -2, width: g(4.5), height: g(4.5), borderRadius: g(2.25), background: t.red, display: "grid", placeItems: "center", border: `2px solid ${t.bg === "#000000" ? "#1a1a2e" : "#e8ecf4"}` }}>
        <span style={{ fontSize: 10, fontWeight: 700, color: "#fff" }}>2</span>
      </div>
    )}
  </div>
);

/* ── Overlay: Slide-up Panel ── */
const PanelOverlay = ({ state, flavor, t }) => (
  <div style={{
    position: "absolute", bottom: 0, left: 0, right: 0, borderRadius: `${g(4)}px ${g(4)}px 0 0`,
    background: t.bg === "#000000" ? "rgba(28,28,30,0.92)" : "rgba(242,242,247,0.92)",
    backdropFilter: "blur(40px)", display: "flex", flexDirection: "column", overflow: "hidden",
  }}>
    {/* Grab handle */}
    <div style={{ padding: `${g(2)}px 0 ${g(1)}px`, display: "flex", justifyContent: "center" }}>
      <div style={{ width: g(9), height: 5, borderRadius: 3, background: t.surface3 }} />
    </div>
    {/* Header */}
    <div style={{ display: "flex", alignItems: "center", gap: g(2.5), padding: `${g(1.5)}px ${g(4)}px ${g(2.5)}px` }}>
      <Orb color={orbClr(flavor, t)} inner={orbInn(flavor, t)} size={g(7)} />
      <span style={{ fontSize: 15, fontWeight: 600, flex: 1, letterSpacing: -0.2, color: t.labelPrimary }}>Fawx</span>
      <button style={{ background: t.surface2, border: "none", borderRadius: g(3.5), padding: `${g(1.25)}px ${g(3)}px`, fontSize: 13, color: t.labelSecondary, cursor: "pointer" }}>Expand</button>
    </div>
    {/* Message */}
    <div style={{ padding: `0 ${g(4)}px ${g(2)}px` }}>
      {state === "executing" ? (
        <div style={{ display: "flex", alignItems: "center", gap: g(2) }}>
          <div style={{ width: 5, height: 5, borderRadius: 2.5, background: orbClr(flavor, t), opacity: 0.6 }} />
          <span style={{ fontSize: 14, color: t.labelSecondary, fontStyle: "italic", letterSpacing: -0.1 }}>Opening calendar...</span>
          <div style={{ flex: 1 }} />
          <button style={{ background: t.red, border: "none", borderRadius: g(3), padding: `${g(1)}px ${g(2.5)}px`, fontSize: 12, fontWeight: 600, color: "#fff", cursor: "pointer" }}>Stop</button>
        </div>
      ) : state === "completed" ? (
        <>
          <div style={{ padding: `${g(2)}px ${g(3)}px`, borderRadius: `${g(3.5)}px ${g(3.5)}px ${g(3.5)}px ${g(1)}px`, background: t.surface2, fontSize: 14, color: t.labelPrimary, lineHeight: "20px", letterSpacing: -0.2, marginBottom: g(2) }}>
            Reminder set for 3:00 PM
          </div>
          <div style={{ display: "inline-flex", alignItems: "center", gap: g(1.5), padding: `${g(1.25)}px ${g(2.5)}px`, borderRadius: g(3), background: `${t.green}18` }}>
            <svg width="10" height="10" viewBox="0 0 10 10" fill="none"><path d="M2 5.5L4.2 7.5 8 3" stroke={t.green} strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round"/></svg>
            <span style={{ fontSize: 12, fontWeight: 500, color: t.green }}>Completed</span>
          </div>
        </>
      ) : state === "failed" ? (
        <div style={{ padding: `${g(2)}px ${g(3)}px`, borderRadius: g(3), background: `${t.red}12`, border: `1px solid ${t.red}22`, fontSize: 14, color: t.labelPrimary, lineHeight: "20px" }}>
          Calendar access denied. Tap to open settings.
        </div>
      ) : (
        <span style={{ fontSize: 14, color: t.labelTertiary, letterSpacing: -0.1 }}>Ready</span>
      )}
    </div>
    {/* Input */}
    <div style={{ padding: `${g(2)}px ${g(3)}px ${g(6)}px`, display: "flex", gap: g(2), borderTop: `0.5px solid ${t.separator}` }}>
      <div style={{ flex: 1, padding: `${g(2.25)}px ${g(3.5)}px`, borderRadius: g(5.5), background: t.surface2, fontSize: 14, color: t.labelTertiary, letterSpacing: -0.2 }}>Message</div>
      <div style={{ width: g(8.5), height: g(8.5), borderRadius: g(4.25), background: sendBg(flavor, t), display: "grid", placeItems: "center" }}>
        {SendIcon(sendIc(flavor, t))}
      </div>
    </div>
  </div>
);

/* ── Overlay: Dynamic Island ── */
const IslandOverlay = ({ state, flavor, t }) => {
  const isExpanded = state !== "idle";
  return (
    <div style={{
      position: "absolute", top: g(3), left: "50%", transform: "translateX(-50%)",
      display: "flex", alignItems: "center", gap: g(2.5),
      padding: isExpanded ? `${g(2)}px ${g(3.5)}px ${g(2)}px ${g(2.5)}px` : `${g(1.5)}px ${g(3)}px ${g(1.5)}px ${g(2)}px`,
      borderRadius: g(7),
      background: "rgba(0,0,0,0.85)", backdropFilter: "blur(40px)",
      boxShadow: "0 4px 20px rgba(0,0,0,0.4)",
      minWidth: isExpanded ? 240 : 120,
      transition: "all 250ms ease",
    }}>
      <Orb
        color={state === "failed" ? t.red : orbClr(flavor, t)}
        inner={state === "failed" ? "rgba(255,255,255,0.2)" : orbInn(flavor, t)}
        size={isExpanded ? g(8) : g(7)}
      />
      {state === "idle" && (
        <span style={{ fontSize: 14, fontWeight: 500, color: "#fff", letterSpacing: -0.2 }}>Fawx</span>
      )}
      {state === "executing" && (
        <>
          <div style={{ flex: 1 }}>
            <span style={{ fontSize: 13, fontWeight: 600, color: "#fff" }}>Opening Calendar...</span>
          </div>
          <button style={{ background: t.red, border: "none", borderRadius: g(3), padding: `${g(1)}px ${g(2.5)}px`, fontSize: 12, fontWeight: 600, color: "#fff", cursor: "pointer" }}>Stop</button>
        </>
      )}
      {state === "completed" && (
        <>
          <div style={{ flex: 1 }}>
            <div style={{ fontSize: 13, fontWeight: 600, color: "#fff" }}>Reminder set</div>
            <div style={{ fontSize: 11, color: "rgba(255,255,255,0.5)", marginTop: 1 }}>Meeting with Sarah · 3:00 PM</div>
          </div>
          <div style={{ width: g(5.5), height: g(5.5), borderRadius: g(2.75), background: t.green, display: "grid", placeItems: "center" }}>
            <svg width="10" height="10" viewBox="0 0 10 10" fill="none"><path d="M2 5.5L4.2 7.5 8 3" stroke="#fff" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round"/></svg>
          </div>
        </>
      )}
      {state === "failed" && (
        <div style={{ flex: 1 }}>
          <div style={{ fontSize: 13, fontWeight: 600, color: "#fff" }}>Calendar access denied</div>
          <div style={{ fontSize: 11, color: "rgba(255,255,255,0.5)", marginTop: 1 }}>Tap to open settings</div>
        </div>
      )}
    </div>
  );
};

/* ── Main ── */
export default function OverlayScreen() {
  const [flavor, setFlavor] = useState("tangerine");
  const [theme, setTheme] = useState("dark");
  const [mode, setMode] = useState("island"); // bubble | panel | island
  const [state, setState] = useState("completed"); // idle | executing | completed | failed | unread
  const t = themes[theme];
  const ct = themes.dark;

  return (
    <div style={{ fontFamily: "-apple-system, 'SF Pro Text', system-ui, sans-serif", background: "#111", minHeight: "100vh", display: "flex", flexDirection: "column", alignItems: "center", padding: `${g(8)}px ${g(4)}px`, WebkitFontSmoothing: "antialiased", color: ct.labelPrimary }}>
      <div style={{ width: "100%", maxWidth: 460, marginBottom: g(5) }}>
        <h1 style={{ fontSize: 24, fontWeight: 700, letterSpacing: -0.6, margin: 0 }}>Overlay Modes</h1>
        <p style={{ fontSize: 15, color: ct.labelSecondary, letterSpacing: -0.2, margin: `${g(1.5)}px 0 ${g(5)}px`, lineHeight: "22px" }}>
          How Fawx appears floating over other apps.
        </p>

        {/* Controls */}
        <div style={{ display: "flex", gap: g(4), flexWrap: "wrap", marginBottom: g(5) }}>
          <div>
            <div style={{ fontSize: 11, fontWeight: 600, color: ct.labelTertiary, letterSpacing: 0.3, textTransform: "uppercase", marginBottom: g(2) }}>Mode</div>
            <div style={{ display: "flex", gap: g(1.5) }}>
              {["bubble", "panel", "island"].map(m => (
                <button key={m} onClick={() => setMode(m)} style={{ padding: `${g(1.5)}px ${g(3)}px`, borderRadius: g(2), background: mode === m ? ct.surface3 : "transparent", border: "none", fontSize: 12, fontWeight: 500, cursor: "pointer", color: mode === m ? ct.labelPrimary : ct.labelTertiary, textTransform: "capitalize" }}>{m === "island" ? "Dynamic Island" : m}</button>
              ))}
            </div>
          </div>
          <div>
            <div style={{ fontSize: 11, fontWeight: 600, color: ct.labelTertiary, letterSpacing: 0.3, textTransform: "uppercase", marginBottom: g(2) }}>State</div>
            <div style={{ display: "flex", gap: g(1.5), flexWrap: "wrap" }}>
              {["idle", "executing", "completed", "failed", ...(mode === "bubble" ? ["unread"] : [])].map(s => (
                <button key={s} onClick={() => setState(s)} style={{ padding: `${g(1.5)}px ${g(3)}px`, borderRadius: g(2), background: state === s ? ct.surface3 : "transparent", border: "none", fontSize: 12, fontWeight: 500, cursor: "pointer", color: state === s ? ct.labelPrimary : ct.labelTertiary, textTransform: "capitalize" }}>{s}</button>
              ))}
            </div>
          </div>
          <div>
            <div style={{ fontSize: 11, fontWeight: 600, color: ct.labelTertiary, letterSpacing: 0.3, textTransform: "uppercase", marginBottom: g(2) }}>Theme</div>
            <div style={{ display: "flex", gap: g(1.5) }}>
              {["dark", "light"].map(th => (
                <button key={th} onClick={() => setTheme(th)} style={{ padding: `${g(1.5)}px ${g(3)}px`, borderRadius: g(2), background: theme === th ? ct.surface3 : "transparent", border: "none", fontSize: 12, fontWeight: 500, cursor: "pointer", color: theme === th ? ct.labelPrimary : ct.labelTertiary, textTransform: "capitalize" }}>{th}</button>
              ))}
            </div>
          </div>
          <div>
            <div style={{ fontSize: 11, fontWeight: 600, color: ct.labelTertiary, letterSpacing: 0.3, textTransform: "uppercase", marginBottom: g(2) }}>Flavor</div>
            <div style={{ display: "flex", gap: g(2) }}>
              <button onClick={() => setFlavor("none")} style={{ width: g(6), height: g(6), borderRadius: g(3), cursor: "pointer", background: "linear-gradient(135deg, #fff 50%, #000 50%)", border: flavor === "none" ? `2px solid ${ct.labelPrimary}` : "2px solid transparent" }} />
              {Object.entries(FLAVORS).filter(([k]) => k !== "none").map(([n, fl]) => (
                <button key={n} onClick={() => setFlavor(n)} style={{ width: g(6), height: g(6), borderRadius: g(3), background: fl.primary, cursor: "pointer", border: flavor === n ? `2px solid ${ct.labelPrimary}` : "2px solid transparent" }} />
              ))}
            </div>
          </div>
        </div>
      </div>

      {/* Phone with overlay */}
      <PhoneBackdrop t={t}>
        {mode === "bubble" && <BubbleOverlay state={state} flavor={flavor} t={t} />}
        {mode === "panel" && <PanelOverlay state={state} flavor={flavor} t={t} />}
        {mode === "island" && <IslandOverlay state={state} flavor={flavor} t={t} />}
      </PhoneBackdrop>
    </div>
  );
}