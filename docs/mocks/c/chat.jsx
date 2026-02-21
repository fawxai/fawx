import { useState, useRef } from "react";

/*
 * ╔══════════════════════════════════════════════════════════════════╗
 * ║  DIRECTIVE C — iOS-Minimal + Orb Presence                       ║
 * ║                                                                  ║
 * ║  The current iOS-Minimal approach with exactly two additions     ║
 * ║  stolen from the alternative directives:                         ║
 * ║                                                                  ║
 * ║  1. Orb glow (from Glass Depth):                                ║
 * ║     box-shadow: 0 0 24px 10px rgba(flavor, 0.15)                ║
 * ║     Gives the orb a sense of luminous presence without blur.     ║
 * ║     Costs ~0 GPU — just a shadow pass.                          ║
 * ║     Applied everywhere the orb appears.                         ║
 * ║     "No flavor" orb gets a very subtle neutral glow.            ║
 * ║                                                                  ║
 * ║  2. Empty-state flavor wash (from Material You):                 ║
 * ║     A radial gradient at 3% opacity behind the orb on the       ║
 * ║     "How can I help?" screen ONLY. Creates ambient warmth.      ║
 * ║     Disappears the instant a conversation starts.               ║
 * ║     "No flavor" gets no wash (stays pure neutral).              ║
 * ║                                                                  ║
 * ║  Everything else is identical to the current approach:           ║
 * ║   • Opaque stepped surfaces                                    ║
 * ║   • Three-touch-point flavor (orb, user bubbles, send)         ║
 * ║   • "No Flavor" fully supported                                ║
 * ║   • 4pt grid, SF Pro, iOS system colors                        ║
 * ╚══════════════════════════════════════════════════════════════════╝
 */

const GRID = 4;
const g = (n) => n * GRID;

const themes = {
  dark: {
    bg: "#000000", surface1: "#1C1C1E", surface2: "#2C2C2E", surface3: "#3A3A3C", surface4: "#48484A",
    labelPrimary: "#FFFFFF", labelSecondary: "rgba(235,235,245,0.60)",
    labelTertiary: "rgba(235,235,245,0.30)", labelQuaternary: "rgba(235,235,245,0.18)",
    separator: "rgba(84,84,88,0.36)", green: "#30D158",
    noFlavorOrb: "#FFFFFF", noFlavorOrbInner: "rgba(0,0,0,0.12)",
    noFlavorBubble: "#48484A", noFlavorSend: "#3A3A3C", noFlavorSendIcon: "rgba(235,235,245,0.60)",
    /* C additions: glow tokens */
    noFlavorGlow: "rgba(255,255,255,0.06)",
  },
  light: {
    bg: "#FFFFFF", surface1: "#F2F2F7", surface2: "#E5E5EA", surface3: "#D1D1D6", surface4: "#C7C7CC",
    labelPrimary: "#000000", labelSecondary: "rgba(60,60,67,0.60)",
    labelTertiary: "rgba(60,60,67,0.30)", labelQuaternary: "rgba(60,60,67,0.18)",
    separator: "rgba(60,60,67,0.12)", green: "#34C759",
    noFlavorOrb: "#000000", noFlavorOrbInner: "rgba(255,255,255,0.20)",
    noFlavorBubble: "#C7C7CC", noFlavorSend: "#D1D1D6", noFlavorSendIcon: "rgba(60,60,67,0.60)",
    /* C additions: glow tokens */
    noFlavorGlow: "rgba(0,0,0,0.04)",
  },
};

const FLAVORS = {
  none:         { primary: null, onPrimary: null, glow: null, wash: null, label: "None" },
  lemon:        { primary: "#FFD600", onPrimary: "#1C1A00", glow: "rgba(255,214,0,0.15)", wash: "rgba(255,214,0,0.03)", label: "Lemon" },
  tangerine:    { primary: "#FF8C00", onPrimary: "#FFFFFF", glow: "rgba(255,140,0,0.15)", wash: "rgba(255,140,0,0.03)", label: "Tangerine" },
  lime:         { primary: "#7CB342", onPrimary: "#FFFFFF", glow: "rgba(124,179,66,0.15)", wash: "rgba(124,179,66,0.03)", label: "Lime" },
  blood_orange: { primary: "#D84315", onPrimary: "#FFFFFF", glow: "rgba(216,67,21,0.15)", wash: "rgba(216,67,21,0.03)", label: "Blood Orange" },
  grapefruit:   { primary: "#E91E63", onPrimary: "#FFFFFF", glow: "rgba(233,30,99,0.15)", wash: "rgba(233,30,99,0.03)", label: "Grapefruit" },
};

const resolve = (flavorKey, t) => {
  const f = FLAVORS[flavorKey];
  if (flavorKey === "none") return {
    orbColor: t.noFlavorOrb, orbInner: t.noFlavorOrbInner,
    orbGlow: t.noFlavorGlow,       /* ← C: subtle neutral glow */
    emptyWash: null,                /* ← C: no wash for "none" */
    bubbleBg: t.noFlavorBubble, bubbleText: t.labelPrimary,
    sendBg: t.noFlavorSend, sendIcon: t.noFlavorSendIcon,
    caretColor: t.labelSecondary, actionIcon: t.labelTertiary,
  };
  return {
    orbColor: f.primary, orbInner: "rgba(0,0,0,0.12)",
    orbGlow: f.glow,                /* ← C: flavor glow */
    emptyWash: f.wash,              /* ← C: 3% flavor wash */
    bubbleBg: f.primary, bubbleText: f.onPrimary,
    sendBg: f.primary, sendIcon: f.onPrimary,
    caretColor: f.primary, actionIcon: t.labelTertiary,
  };
};

const Icons = {
  settings: (c) => <svg width="20" height="20" viewBox="0 0 20 20" fill="none"><circle cx="10" cy="10" r="2.5" stroke={c} strokeWidth="1.5"/><path d="M10 2.5v1.5M10 16v1.5M2.5 10H4M16 10h1.5M4.4 4.4l1.06 1.06M14.54 14.54l1.06 1.06M4.4 15.6l1.06-1.06M14.54 5.46l1.06-1.06" stroke={c} strokeWidth="1.5" strokeLinecap="round"/></svg>,
  send: (c) => <svg width="16" height="16" viewBox="0 0 16 16" fill="none"><path d="M8 13V3M3.5 7.5L8 3l4.5 4.5" stroke={c} strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"/></svg>,
  mic: (c) => <svg width="18" height="18" viewBox="0 0 18 18" fill="none"><rect x="6.5" y="2" width="5" height="9" rx="2.5" stroke={c} strokeWidth="1.5"/><path d="M4 9.5a5 5 0 0010 0M9 14.5v2" stroke={c} strokeWidth="1.5" strokeLinecap="round"/></svg>,
  cal: (c) => <svg width="14" height="14" viewBox="0 0 14 14" fill="none"><rect x="1.5" y="2.5" width="11" height="10" rx="1.5" stroke={c} strokeWidth="1.2"/><path d="M1.5 5.5h11M4.5 1v2.5M9.5 1v2.5" stroke={c} strokeWidth="1.2" strokeLinecap="round"/></svg>,
  check: (c) => <svg width="12" height="12" viewBox="0 0 12 12" fill="none"><path d="M2.5 6.5L5 9l4.5-6" stroke={c} strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/></svg>,
};

/* ── Orb — now with box-shadow glow ── */
const Orb = ({ color, inner = "rgba(0,0,0,0.12)", glow, size = 32 }) => {
  const r = Math.round(size * 0.38);
  return (
    <div style={{
      width: size, height: size, borderRadius: size / 2,
      background: color, display: "grid", placeItems: "center", flexShrink: 0,
      /* ▼ DIRECTIVE C ADDITION: soft glow */
      boxShadow: glow ? `0 0 ${Math.round(size * 0.45)}px ${Math.round(size * 0.18)}px ${glow}` : "none",
      transition: "box-shadow 200ms ease",
    }}>
      <div style={{ width: r, height: r, borderRadius: r / 2, background: inner }} />
    </div>
  );
};

const Bubble = ({ role, text, meta, r, t }) => {
  const isUser = role === "user";
  if (role === "action") return (
    <div style={{ display: "flex", alignItems: "center", gap: g(2), padding: `${g(1)}px 0`, alignSelf: "flex-start" }}>
      {Icons.cal(r.actionIcon)}
      <span style={{ fontSize: 13, letterSpacing: -0.1, color: t.labelSecondary, lineHeight: "18px" }}>{text}</span>
      {Icons.check(t.green)}
    </div>
  );
  return (
    <div style={{ alignSelf: isUser ? "flex-end" : "flex-start", maxWidth: "82%" }}>
      <div style={{
        padding: `${g(2.5)}px ${g(3.5)}px`,
        borderRadius: isUser ? "18px 18px 4px 18px" : "18px 18px 18px 4px",
        background: isUser ? r.bubbleBg : t.surface2,
        color: isUser ? r.bubbleText : t.labelPrimary,
        fontSize: 16, lineHeight: "22px", letterSpacing: -0.2,
      }}>{text}</div>
      {meta && <div style={{ fontSize: 12, lineHeight: "16px", letterSpacing: -0.1, color: t.labelTertiary, marginTop: g(1), paddingInline: g(1.5), textAlign: isUser ? "right" : "left" }}>{meta}</div>}
    </div>
  );
};

const MESSAGES = [
  { role: "user", text: "Set a reminder for my 3pm meeting with Sarah" },
  { role: "action", text: "Reminder created — 3:00 PM" },
  { role: "assistant", text: "Done. You'll get a notification 10 minutes before. Want me to draft a quick agenda?" },
  { role: "user", text: "Yes, keep it short. Three bullet points max." },
  { role: "assistant", text: "Here's a draft agenda:\n\n• Q1 metrics review\n• Product roadmap priorities\n• Engineering hiring timeline\n\nI can add it to the calendar event if you'd like." },
  { role: "user", text: "Do it" },
  { role: "action", text: "Calendar event updated" },
  { role: "assistant", text: "Updated. Sarah will see the agenda when she opens the invite." },
];

const SUGGESTIONS = ["What's on my calendar today?", "Read my unread messages", "Set DND until 5pm"];

/* ── Controls ── */
const Controls = ({ flavor, setFlavor, theme, setTheme, showEmpty, setShowEmpty }) => {
  const ct = themes.dark;
  return (
    <div style={{ position: "fixed", top: g(4), right: g(4), zIndex: 10 }}>
      <div style={{ background: ct.surface1, borderRadius: g(3), padding: g(3), display: "flex", flexDirection: "column", gap: g(3), color: ct.labelPrimary }}>
        <div>
          <div style={{ fontSize: 11, fontWeight: 600, color: ct.labelTertiary, letterSpacing: 0.5, textTransform: "uppercase", marginBottom: g(2) }}>Theme</div>
          <div style={{ display: "flex", gap: g(1.5) }}>
            {["dark", "light"].map(th => (
              <button key={th} onClick={() => setTheme(th)} style={{
                padding: `${g(1.5)}px ${g(3)}px`, borderRadius: g(2), background: theme === th ? ct.surface3 : "transparent",
                border: "none", fontSize: 12, fontWeight: 500, cursor: "pointer", color: theme === th ? ct.labelPrimary : ct.labelTertiary, textTransform: "capitalize",
              }}>{th}</button>
            ))}
          </div>
        </div>
        <div>
          <div style={{ fontSize: 11, fontWeight: 600, color: ct.labelTertiary, letterSpacing: 0.5, textTransform: "uppercase", marginBottom: g(2) }}>Flavor</div>
          <div style={{ display: "flex", gap: g(2), alignItems: "center" }}>
            <button onClick={() => setFlavor("none")} style={{ width: g(7), height: g(7), borderRadius: g(3.5), cursor: "pointer", background: "linear-gradient(135deg, #fff 50%, #000 50%)", border: flavor === "none" ? `2px solid ${ct.labelPrimary}` : "2px solid transparent" }} aria-label="None" />
            {Object.entries(FLAVORS).filter(([k]) => k !== "none").map(([n, fl]) => (
              <button key={n} onClick={() => setFlavor(n)} style={{ width: g(7), height: g(7), borderRadius: g(3.5), background: fl.primary, cursor: "pointer", border: flavor === n ? `2px solid ${ct.labelPrimary}` : "2px solid transparent" }} aria-label={fl.label} />
            ))}
          </div>
        </div>
        <div style={{ display: "flex", gap: g(1.5) }}>
          {[["Conversation", false], ["Empty", true]].map(([label, val]) => (
            <button key={label} onClick={() => setShowEmpty(val)} style={{
              padding: `${g(1.5)}px ${g(3)}px`, borderRadius: g(2), background: showEmpty === val ? ct.surface3 : "transparent",
              border: "none", fontSize: 12, fontWeight: 500, cursor: "pointer", color: showEmpty === val ? ct.labelPrimary : ct.labelTertiary,
            }}>{label}</button>
          ))}
        </div>
      </div>
    </div>
  );
};

export default function DirectiveCChat() {
  const [flavor, setFlavor] = useState("tangerine");
  const [theme, setTheme] = useState("dark");
  const [inputValue, setInputValue] = useState("");
  const [showEmpty, setShowEmpty] = useState(true); /* default to empty to showcase the wash */
  const inputRef = useRef(null);
  const t = themes[theme];
  const r = resolve(flavor, t);
  const hasText = inputValue.trim().length > 0;

  return (
    <div style={{ fontFamily: "-apple-system, 'SF Pro Text', system-ui, sans-serif", background: t.bg, width: 393, minHeight: 852, display: "flex", flexDirection: "column", margin: "0 auto", WebkitFontSmoothing: "antialiased", transition: "background 200ms ease" }}>
      <Controls flavor={flavor} setFlavor={setFlavor} theme={theme} setTheme={setTheme} showEmpty={showEmpty} setShowEmpty={setShowEmpty} />

      {/* Nav — identical to current, but orb gets glow */}
      <nav style={{ display: "flex", alignItems: "center", padding: `${g(3)}px ${g(4)}px`, gap: g(3), borderBottom: `0.5px solid ${t.separator}`, height: g(11), flexShrink: 0 }}>
        <Orb color={r.orbColor} inner={r.orbInner} glow={r.orbGlow} size={g(8)} />
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ fontSize: 17, fontWeight: 600, letterSpacing: -0.4, color: t.labelPrimary, lineHeight: "22px" }}>Citros</div>
          <div style={{ fontSize: 13, letterSpacing: -0.1, color: t.labelTertiary, lineHeight: "18px" }}>Claude Sonnet 4.5</div>
        </div>
        <button style={{ width: g(11), height: g(11), display: "grid", placeItems: "center", background: "none", border: "none", cursor: "pointer", borderRadius: g(3) }} aria-label="Settings">
          {Icons.settings(t.labelSecondary)}
        </button>
      </nav>

      {/* Messages */}
      <div style={{ flex: 1, overflowY: "auto", padding: `${g(4)}px ${g(4)}px ${g(2)}px`, display: "flex", flexDirection: "column", gap: g(2) }}>
        {showEmpty ? (
          /* ▼ DIRECTIVE C ADDITION: empty state has a radial flavor wash */
          <div style={{
            flex: 1, display: "flex", flexDirection: "column",
            alignItems: "center", justifyContent: "center",
            gap: g(5), paddingBottom: g(16),
            /* The wash: radial gradient from orb center, 3% flavor opacity */
            background: r.emptyWash
              ? `radial-gradient(ellipse 70% 45% at 50% 40%, ${r.emptyWash}, transparent 80%)`
              : "none",
            transition: "background 300ms ease",
          }}>
            <Orb color={r.orbColor} inner={r.orbInner} glow={r.orbGlow} size={g(14)} />
            <div style={{ fontSize: 20, fontWeight: 600, letterSpacing: -0.4, color: t.labelPrimary }}>How can I help?</div>
            <div style={{ display: "flex", flexWrap: "wrap", gap: g(2), justifyContent: "center", padding: `0 ${g(4)}px` }}>
              {SUGGESTIONS.map((s, i) => (
                <button key={i} style={{ padding: `${g(2.5)}px ${g(4)}px`, borderRadius: g(5), background: t.surface2, border: "none", fontSize: 14, letterSpacing: -0.15, color: t.labelSecondary, cursor: "pointer", lineHeight: "20px" }}>{s}</button>
              ))}
            </div>
          </div>
        ) : (
          MESSAGES.map((msg, i) => <Bubble key={i} role={msg.role} text={msg.text} r={r} t={t} />)
        )}
      </div>

      {/* Input — identical to current */}
      <div style={{ padding: `${g(2)}px ${g(3)}px ${g(8.5)}px`, borderTop: `0.5px solid ${t.separator}`, flexShrink: 0 }}>
        <div style={{ display: "flex", alignItems: "flex-end", gap: g(2) }}>
          <div style={{ flex: 1, background: t.surface2, borderRadius: g(6), display: "flex", alignItems: "flex-end", minHeight: g(10), padding: `0 ${g(1)}px 0 ${g(4)}px` }}>
            <input ref={inputRef} type="text" value={inputValue} onChange={e => setInputValue(e.target.value)} placeholder="Message" style={{ flex: 1, background: "none", border: "none", outline: "none", fontSize: 16, letterSpacing: -0.2, lineHeight: "22px", color: t.labelPrimary, padding: `${g(2.25)}px 0`, caretColor: r.caretColor }} />
            {!hasText && <button style={{ width: g(10), height: g(10), display: "grid", placeItems: "center", background: "none", border: "none", cursor: "pointer", flexShrink: 0, opacity: 0.6 }} aria-label="Voice">{Icons.mic(t.labelSecondary)}</button>}
          </div>
          <button style={{ width: g(9), height: g(9), borderRadius: g(4.5), background: hasText ? r.sendBg : t.surface3, border: "none", cursor: hasText ? "pointer" : "default", display: "grid", placeItems: "center", flexShrink: 0, transition: "background 150ms ease", marginBottom: g(0.25) }} aria-label="Send">
            {Icons.send(hasText ? r.sendIcon : t.labelQuaternary)}
          </button>
        </div>
      </div>
    </div>
  );
}