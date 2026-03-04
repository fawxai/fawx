import { useState, useRef } from "react";

/*
 * ╔══════════════════════════════════════════════════════════════════╗
 * ║  DIRECTIVE B — Translucent Depth / visionOS Glass               ║
 * ║                                                                  ║
 * ║  Philosophy: Apple's spatial-era aesthetic — frosted glass        ║
 * ║  surfaces, layered depth through blur + transparency, and a      ║
 * ║  reduced role for solid color. The flavor manifests as a soft    ║
 * ║  ambient glow behind the orb and a tinted frosted-glass          ║
 * ║  treatment on user bubbles. Everything else is blur + alpha.     ║
 * ║                                                                  ║
 * ║  Key differences from current approach:                          ║
 * ║   • Frosted glass (backdrop-filter: blur) on ALL containers      ║
 * ║   • Background mesh gradient tinted by flavor                    ║
 * ║   • User bubbles: tinted glass, not solid color                  ║
 * ║   • Nav bar: transparent with blur underlay                      ║
 * ║   • Depth via layered translucency, not surface color steps      ║
 * ║   • Orb has ambient glow (box-shadow with flavor color)          ║
 * ║   • Typography: SF Pro, same as current — but lighter weights    ║
 * ║   • "No flavor" = fully neutral gray glass, no tinting           ║
 * ║   • 4pt grid maintained — spatial depth, not spatial grid change ║
 * ╚══════════════════════════════════════════════════════════════════╝
 */

const GRID = 4;
const g = (n) => n * GRID;

const FLAVORS = {
  none:         { primary: null, glow: null, tint: null, label: "None" },
  lemon:        { primary: "#FFD600", glow: "rgba(255,214,0,0.30)", tint: "rgba(255,214,0,0.12)", label: "Lemon" },
  tangerine:    { primary: "#FF8C00", glow: "rgba(255,140,0,0.30)", tint: "rgba(255,140,0,0.12)", label: "Tangerine" },
  lime:         { primary: "#7CB342", glow: "rgba(124,179,66,0.30)", tint: "rgba(124,179,66,0.12)", label: "Lime" },
  blood_orange: { primary: "#D84315", glow: "rgba(216,67,21,0.30)", tint: "rgba(216,67,21,0.12)", label: "Blood Orange" },
  grapefruit:   { primary: "#E91E63", glow: "rgba(233,30,99,0.30)", tint: "rgba(233,30,99,0.12)", label: "Grapefruit" },
};

const themes = {
  dark: {
    textPrimary: "#FFFFFF",
    textSecondary: "rgba(235,235,245,0.60)",
    textTertiary: "rgba(235,235,245,0.30)",
    glass: "rgba(255,255,255,0.06)",
    glassHigh: "rgba(255,255,255,0.10)",
    glassBorder: "rgba(255,255,255,0.08)",
    inputGlass: "rgba(255,255,255,0.08)",
    separator: "rgba(255,255,255,0.06)",
    meshBase: "#0A0A0F",
    meshMid: "#111118",
    noFlavorOrb: "#FFFFFF",
    noFlavorGlow: "rgba(255,255,255,0.08)",
    noFlavorTint: "rgba(255,255,255,0.06)",
    noFlavorBubble: "rgba(255,255,255,0.12)",
    green: "#30D158",
  },
  light: {
    textPrimary: "#000000",
    textSecondary: "rgba(60,60,67,0.60)",
    textTertiary: "rgba(60,60,67,0.30)",
    glass: "rgba(255,255,255,0.50)",
    glassHigh: "rgba(255,255,255,0.70)",
    glassBorder: "rgba(255,255,255,0.40)",
    inputGlass: "rgba(0,0,0,0.04)",
    separator: "rgba(0,0,0,0.06)",
    meshBase: "#F0F0F5",
    meshMid: "#E8E8F0",
    noFlavorOrb: "#000000",
    noFlavorGlow: "rgba(0,0,0,0.06)",
    noFlavorTint: "rgba(0,0,0,0.04)",
    noFlavorBubble: "rgba(0,0,0,0.06)",
    green: "#34C759",
  },
};

const resolveGlass = (flavorKey, t) => {
  const f = FLAVORS[flavorKey];
  if (flavorKey === "none") return {
    orbColor: t.noFlavorOrb, orbGlow: t.noFlavorGlow,
    userBubble: t.noFlavorBubble, userText: t.textPrimary,
    sendBg: t.noFlavorTint, sendBorder: t.glassBorder,
    sendIcon: t.textSecondary,
    meshAccent: "transparent",
  };
  return {
    orbColor: f.primary, orbGlow: f.glow,
    userBubble: f.tint, userText: t.textPrimary,
    sendBg: f.tint, sendBorder: `${f.primary}33`,
    sendIcon: f.primary,
    meshAccent: f.glow,
  };
};

const Icons = {
  settings: (c) => <svg width="20" height="20" viewBox="0 0 20 20" fill="none"><circle cx="10" cy="10" r="2.5" stroke={c} strokeWidth="1.5"/><path d="M10 2.5v1.5M10 16v1.5M2.5 10H4M16 10h1.5M4.4 4.4l1.06 1.06M14.54 14.54l1.06 1.06M4.4 15.6l1.06-1.06M14.54 5.46l1.06-1.06" stroke={c} strokeWidth="1.5" strokeLinecap="round"/></svg>,
  send: (c) => <svg width="16" height="16" viewBox="0 0 16 16" fill="none"><path d="M8 13V3M3.5 7.5L8 3l4.5 4.5" stroke={c} strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"/></svg>,
  mic: (c) => <svg width="18" height="18" viewBox="0 0 18 18" fill="none"><rect x="6.5" y="2" width="5" height="9" rx="2.5" stroke={c} strokeWidth="1.5"/><path d="M4 9.5a5 5 0 0010 0M9 14.5v2" stroke={c} strokeWidth="1.5" strokeLinecap="round"/></svg>,
  cal: (c) => <svg width="14" height="14" viewBox="0 0 14 14" fill="none"><rect x="1.5" y="2.5" width="11" height="10" rx="1.5" stroke={c} strokeWidth="1.2"/><path d="M1.5 5.5h11M4.5 1v2.5M9.5 1v2.5" stroke={c} strokeWidth="1.2" strokeLinecap="round"/></svg>,
  check: (c) => <svg width="12" height="12" viewBox="0 0 12 12" fill="none"><path d="M2.5 6.5L5 9l4.5-6" stroke={c} strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/></svg>,
};

const Orb = ({ color, glow, size = 32 }) => {
  const r = Math.round(size * 0.38);
  return (
    <div style={{
      width: size, height: size, borderRadius: size / 2,
      background: color, display: "grid", placeItems: "center", flexShrink: 0,
      boxShadow: glow ? `0 0 ${size * 0.6}px ${size * 0.3}px ${glow}` : "none",
      transition: "box-shadow 300ms ease",
    }}>
      <div style={{ width: r, height: r, borderRadius: r / 2, background: "rgba(0,0,0,0.12)" }} />
    </div>
  );
};

const Bubble = ({ role, text, rv, t }) => {
  const isUser = role === "user";
  if (role === "action") return (
    <div style={{ display: "flex", alignItems: "center", gap: g(2), padding: `${g(1)}px 0`, alignSelf: "flex-start" }}>
      {Icons.cal(t.textTertiary)}
      <span style={{ fontSize: 13, letterSpacing: -0.1, color: t.textSecondary, lineHeight: "18px" }}>{text}</span>
      {Icons.check(t.green)}
    </div>
  );
  return (
    <div style={{ alignSelf: isUser ? "flex-end" : "flex-start", maxWidth: "82%" }}>
      <div style={{
        padding: `${g(2.5)}px ${g(3.5)}px`,
        borderRadius: isUser ? "18px 18px 4px 18px" : "18px 18px 18px 4px",
        background: isUser ? rv.userBubble : t.glass,
        backdropFilter: "blur(40px) saturate(1.8)",
        WebkitBackdropFilter: "blur(40px) saturate(1.8)",
        border: `0.5px solid ${t.glassBorder}`,
        color: isUser ? rv.userText : t.textPrimary,
        fontSize: 16, lineHeight: "22px", letterSpacing: -0.2,
        whiteSpace: "pre-wrap",
      }}>{text}</div>
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

const Controls = ({ flavor, setFlavor, theme, setTheme, showEmpty, setShowEmpty }) => (
  <div style={{ position: "fixed", top: g(4), right: g(4), zIndex: 10 }}>
    <div style={{ background: "#1C1C1E", borderRadius: g(3), padding: g(3), display: "flex", flexDirection: "column", gap: g(3), color: "#fff" }}>
      <div>
        <div style={{ fontSize: 11, fontWeight: 600, color: "rgba(235,235,245,0.30)", letterSpacing: 0.5, textTransform: "uppercase", marginBottom: g(2) }}>Theme</div>
        <div style={{ display: "flex", gap: g(1.5) }}>
          {["dark", "light"].map(th => (
            <button key={th} onClick={() => setTheme(th)} style={{
              padding: `${g(1.5)}px ${g(3)}px`, borderRadius: g(2), background: theme === th ? "#3A3A3C" : "transparent",
              border: "none", fontSize: 12, fontWeight: 500, cursor: "pointer", color: theme === th ? "#fff" : "rgba(235,235,245,0.30)", textTransform: "capitalize",
            }}>{th}</button>
          ))}
        </div>
      </div>
      <div>
        <div style={{ fontSize: 11, fontWeight: 600, color: "rgba(235,235,245,0.30)", letterSpacing: 0.5, textTransform: "uppercase", marginBottom: g(2) }}>Flavor</div>
        <div style={{ display: "flex", gap: g(2), alignItems: "center" }}>
          <button onClick={() => setFlavor("none")} style={{ width: g(7), height: g(7), borderRadius: g(3.5), cursor: "pointer", background: "linear-gradient(135deg, #fff 50%, #000 50%)", border: flavor === "none" ? `2px solid #fff` : "2px solid transparent" }} aria-label="None" />
          {Object.entries(FLAVORS).filter(([k]) => k !== "none").map(([n, fl]) => (
            <button key={n} onClick={() => setFlavor(n)} style={{ width: g(7), height: g(7), borderRadius: g(3.5), background: fl.primary, cursor: "pointer", border: flavor === n ? `2px solid #fff` : "2px solid transparent" }} aria-label={fl.label} />
          ))}
        </div>
      </div>
      <div style={{ display: "flex", gap: g(1.5) }}>
        {[["Chat", false], ["Empty", true]].map(([label, val]) => (
          <button key={label} onClick={() => setShowEmpty(val)} style={{
            padding: `${g(1.5)}px ${g(3)}px`, borderRadius: g(2), background: showEmpty === val ? "#3A3A3C" : "transparent",
            border: "none", fontSize: 12, fontWeight: 500, cursor: "pointer", color: showEmpty === val ? "#fff" : "rgba(235,235,245,0.30)",
          }}>{label}</button>
        ))}
      </div>
    </div>
  </div>
);

export default function GlassDepthChat() {
  const [flavor, setFlavor] = useState("tangerine");
  const [theme, setTheme] = useState("dark");
  const [inputValue, setInputValue] = useState("");
  const [showEmpty, setShowEmpty] = useState(false);
  const inputRef = useRef(null);
  const t = themes[theme];
  const rv = resolveGlass(flavor, t);
  const hasText = inputValue.trim().length > 0;
  const isDark = theme === "dark";

  /* Mesh gradient: flavor glow radiating from center-top */
  const meshGradient = isDark
    ? `radial-gradient(ellipse 80% 50% at 50% 10%, ${rv.meshAccent}, transparent 70%), linear-gradient(180deg, ${t.meshBase} 0%, ${t.meshMid} 100%)`
    : `radial-gradient(ellipse 80% 50% at 50% 10%, ${rv.meshAccent}, transparent 70%), linear-gradient(180deg, ${t.meshMid} 0%, ${t.meshBase} 100%)`;

  return (
    <div style={{
      fontFamily: "-apple-system, 'SF Pro Text', system-ui, sans-serif",
      background: meshGradient,
      width: 393, minHeight: 852,
      display: "flex", flexDirection: "column", margin: "0 auto",
      WebkitFontSmoothing: "antialiased",
      transition: "background 300ms ease",
    }}>
      <Controls flavor={flavor} setFlavor={setFlavor} theme={theme} setTheme={setTheme} showEmpty={showEmpty} setShowEmpty={setShowEmpty} />

      {/* Nav — frosted glass bar */}
      <nav style={{
        display: "flex", alignItems: "center", padding: `${g(3)}px ${g(4)}px`,
        gap: g(3), height: g(11), flexShrink: 0,
        background: t.glass,
        backdropFilter: "blur(40px) saturate(1.8)",
        WebkitBackdropFilter: "blur(40px) saturate(1.8)",
        borderBottom: `0.5px solid ${t.glassBorder}`,
      }}>
        <Orb color={rv.orbColor} glow={rv.orbGlow} size={g(8)} />
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ fontSize: 17, fontWeight: 600, letterSpacing: -0.4, color: t.textPrimary, lineHeight: "22px" }}>Fawx</div>
          <div style={{ fontSize: 13, letterSpacing: -0.1, color: t.textTertiary, lineHeight: "18px" }}>Claude Sonnet 4.5</div>
        </div>
        <button style={{ width: g(11), height: g(11), display: "grid", placeItems: "center", background: "none", border: "none", cursor: "pointer", borderRadius: g(3) }} aria-label="Settings">
          {Icons.settings(t.textSecondary)}
        </button>
      </nav>

      {/* Messages */}
      <div style={{ flex: 1, overflowY: "auto", padding: `${g(4)}px ${g(4)}px ${g(2)}px`, display: "flex", flexDirection: "column", gap: g(2) }}>
        {showEmpty ? (
          <div style={{ flex: 1, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: g(5), paddingBottom: g(16) }}>
            <Orb color={rv.orbColor} glow={rv.orbGlow} size={g(14)} />
            <div style={{ fontSize: 20, fontWeight: 600, letterSpacing: -0.4, color: t.textPrimary }}>How can I help?</div>
            <div style={{ display: "flex", flexWrap: "wrap", gap: g(2), justifyContent: "center", padding: `0 ${g(4)}px` }}>
              {SUGGESTIONS.map((s, i) => (
                <button key={i} style={{
                  padding: `${g(2.5)}px ${g(4)}px`, borderRadius: g(5),
                  background: t.glass,
                  backdropFilter: "blur(40px) saturate(1.8)",
                  WebkitBackdropFilter: "blur(40px) saturate(1.8)",
                  border: `0.5px solid ${t.glassBorder}`,
                  fontSize: 14, letterSpacing: -0.15, color: t.textSecondary,
                  cursor: "pointer", lineHeight: "20px",
                }}>{s}</button>
              ))}
            </div>
          </div>
        ) : (
          MESSAGES.map((msg, i) => <Bubble key={i} role={msg.role} text={msg.text} rv={rv} t={t} />)
        )}
      </div>

      {/* Input — frosted glass */}
      <div style={{
        padding: `${g(2)}px ${g(3)}px ${g(8.5)}px`,
        background: t.glass,
        backdropFilter: "blur(40px) saturate(1.8)",
        WebkitBackdropFilter: "blur(40px) saturate(1.8)",
        borderTop: `0.5px solid ${t.glassBorder}`,
        flexShrink: 0,
      }}>
        <div style={{ display: "flex", alignItems: "flex-end", gap: g(2) }}>
          <div style={{
            flex: 1, background: t.inputGlass, borderRadius: g(6),
            display: "flex", alignItems: "flex-end", minHeight: g(10),
            padding: `0 ${g(1)}px 0 ${g(4)}px`,
            border: `0.5px solid ${t.glassBorder}`,
          }}>
            <input
              ref={inputRef} type="text" value={inputValue}
              onChange={e => setInputValue(e.target.value)}
              placeholder="Message"
              style={{
                flex: 1, background: "none", border: "none", outline: "none",
                fontSize: 16, letterSpacing: -0.2, lineHeight: "22px",
                color: t.textPrimary, padding: `${g(2.25)}px 0`,
                caretColor: rv.orbColor || t.textSecondary,
              }}
            />
            {!hasText && (
              <button style={{ width: g(10), height: g(10), display: "grid", placeItems: "center", background: "none", border: "none", cursor: "pointer", flexShrink: 0, opacity: 0.6 }} aria-label="Voice">
                {Icons.mic(t.textSecondary)}
              </button>
            )}
          </div>
          <button style={{
            width: g(9), height: g(9), borderRadius: g(4.5),
            background: hasText ? rv.sendBg : t.glass,
            backdropFilter: "blur(20px)",
            WebkitBackdropFilter: "blur(20px)",
            border: `0.5px solid ${hasText ? rv.sendBorder : t.glassBorder}`,
            cursor: hasText ? "pointer" : "default",
            display: "grid", placeItems: "center", flexShrink: 0,
            transition: "all 150ms ease", marginBottom: g(0.25),
          }} aria-label="Send">
            {Icons.send(hasText ? rv.sendIcon : t.textTertiary)}
          </button>
        </div>
      </div>
    </div>
  );
}