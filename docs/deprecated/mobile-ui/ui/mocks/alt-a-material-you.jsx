import { useState, useRef } from "react";

/*
 * ╔══════════════════════════════════════════════════════════════════╗
 * ║  DIRECTIVE A — Material You / Dynamic Color                     ║
 * ║                                                                  ║
 * ║  Philosophy: Google's Material 3 — extract a tonal palette from  ║
 * ║  the user's chosen flavor, then flood the entire UI with it.     ║
 * ║  Every surface, container, and interactive element carries the    ║
 * ║  flavor. Personality is *everywhere*, not just three touchpoints. ║
 * ║                                                                  ║
 * ║  Key differences from current approach:                          ║
 * ║   • Full tonal palette (5 luminance steps) from each flavor      ║
 * ║   • Large corner radii (28px containers, 16px chips)             ║
 * ║   • Prominent FAB instead of inline send button                  ║
 * ║   • "On" colors derived from flavor (not static white/black)     ║
 * ║   • Assistant surface uses flavor-tinted container                ║
 * ║   • Typography: Google Sans / Roboto Flex, looser tracking       ║
 * ║   • 8dp base grid (not 4pt)                                      ║
 * ║   • No "no flavor" option — there's always a palette             ║
 * ╚══════════════════════════════════════════════════════════════════╝
 */

const GRID = 8;
const g = (n) => n * GRID;

/* ── Tonal palette generator ── */
/* In real M3, this comes from HCT. Here we approximate 5 tonal steps
   from each flavor hue: surface-dim → surface → surface-bright →
   primary-container → primary. */
const palettes = {
  lemon: {
    primary: "#FFD600", onPrimary: "#3A3000",
    primaryContainer: "#FFF1A0", onPrimaryContainer: "#221B00",
    surface: "#1E1B16", surfaceBright: "#2D2A22", surfaceContainer: "#252218",
    surfaceContainerHigh: "#302D25", surfaceContainerHighest: "#3B382F",
    onSurface: "#ECE6D4", onSurfaceVariant: "#CDC6B1",
    outline: "#968F7E", outlineVariant: "#4A4639",
    // Light variant
    surfaceLight: "#FFF8E1", surfaceBrightLight: "#FFFFFF",
    surfaceContainerLight: "#FFF3CC", surfaceContainerHighLight: "#FFECB0",
    surfaceContainerHighestLight: "#FFE59D",
    onSurfaceLight: "#1E1B16", onSurfaceVariantLight: "#4A4639",
    outlineLight: "#7C7768", outlineVariantLight: "#CDC6B1",
  },
  tangerine: {
    primary: "#FF8C00", onPrimary: "#FFFFFF",
    primaryContainer: "#FFD5A0", onPrimaryContainer: "#2B1700",
    surface: "#1F1B16", surfaceBright: "#2E2921", surfaceContainer: "#262118",
    surfaceContainerHigh: "#312B22", surfaceContainerHighest: "#3C362C",
    onSurface: "#EDE4D4", onSurfaceVariant: "#D0C4AE",
    outline: "#998D7A", outlineVariant: "#4D4336",
    surfaceLight: "#FFF3E0", surfaceBrightLight: "#FFFFFF",
    surfaceContainerLight: "#FFE8CC", surfaceContainerHighLight: "#FFDCB0",
    surfaceContainerHighestLight: "#FFD09A",
    onSurfaceLight: "#1F1B16", onSurfaceVariantLight: "#4D4336",
    outlineLight: "#7F7567", outlineVariantLight: "#D0C4AE",
  },
  lime: {
    primary: "#7CB342", onPrimary: "#FFFFFF",
    primaryContainer: "#C5E99B", onPrimaryContainer: "#0F2000",
    surface: "#1A1E15", surfaceBright: "#272C20", surfaceContainer: "#21261A",
    surfaceContainerHigh: "#2C3124", surfaceContainerHighest: "#373C2F",
    onSurface: "#DEE7CC", onSurfaceVariant: "#C1CAA9",
    outline: "#8B9478", outlineVariant: "#434B38",
    surfaceLight: "#F1F8E9", surfaceBrightLight: "#FFFFFF",
    surfaceContainerLight: "#E2F0D0", surfaceContainerHighLight: "#D4E8B8",
    surfaceContainerHighestLight: "#C5E09F",
    onSurfaceLight: "#1A1E15", onSurfaceVariantLight: "#434B38",
    outlineLight: "#6F7862", outlineVariantLight: "#C1CAA9",
  },
  blood_orange: {
    primary: "#D84315", onPrimary: "#FFFFFF",
    primaryContainer: "#FFAB91", onPrimaryContainer: "#2C0800",
    surface: "#201A17", surfaceBright: "#302824", surfaceContainer: "#27201C",
    surfaceContainerHigh: "#322A26", surfaceContainerHighest: "#3D3531",
    onSurface: "#EDE0DA", onSurfaceVariant: "#D6C3BA",
    outline: "#9E8D84", outlineVariant: "#4F413A",
    surfaceLight: "#FBE9E7", surfaceBrightLight: "#FFFFFF",
    surfaceContainerLight: "#FFDDD2", surfaceContainerHighLight: "#FFD0C0",
    surfaceContainerHighestLight: "#FFC4AD",
    onSurfaceLight: "#201A17", onSurfaceVariantLight: "#4F413A",
    outlineLight: "#827268", outlineVariantLight: "#D6C3BA",
  },
  grapefruit: {
    primary: "#E91E63", onPrimary: "#FFFFFF",
    primaryContainer: "#FFB2C8", onPrimaryContainer: "#3A001D",
    surface: "#201A1C", surfaceBright: "#302729", surfaceContainer: "#272022",
    surfaceContainerHigh: "#322A2C", surfaceContainerHighest: "#3D3537",
    onSurface: "#EDE0E3", onSurfaceVariant: "#D6C1C7",
    outline: "#9E8B92", outlineVariant: "#4F3F44",
    surfaceLight: "#FCE4EC", surfaceBrightLight: "#FFFFFF",
    surfaceContainerLight: "#FFD6E0", surfaceContainerHighLight: "#FFC8D4",
    surfaceContainerHighestLight: "#FFBAC8",
    onSurfaceLight: "#201A1C", onSurfaceVariantLight: "#4F3F44",
    outlineLight: "#827078", outlineVariantLight: "#D6C1C7",
  },
};

const resolveM3 = (flavorKey, isDark) => {
  const p = palettes[flavorKey];
  if (isDark) return {
    bg: p.surface, surface: p.surfaceContainer, surfaceHigh: p.surfaceContainerHigh,
    surfaceHighest: p.surfaceContainerHighest, bright: p.surfaceBright,
    primary: p.primary, onPrimary: p.onPrimary,
    primaryContainer: p.primaryContainer, onPrimaryContainer: p.onPrimaryContainer,
    onSurface: p.onSurface, onSurfaceVariant: p.onSurfaceVariant,
    outline: p.outline, outlineVariant: p.outlineVariant,
  };
  return {
    bg: p.surfaceLight, surface: p.surfaceContainerLight, surfaceHigh: p.surfaceContainerHighLight,
    surfaceHighest: p.surfaceContainerHighestLight, bright: p.surfaceBrightLight,
    primary: p.primary, onPrimary: p.onPrimary,
    primaryContainer: p.primaryContainer, onPrimaryContainer: p.onPrimaryContainer,
    onSurface: p.onSurfaceLight, onSurfaceVariant: p.onSurfaceVariantLight,
    outline: p.outlineLight, outlineVariant: p.outlineVariantLight,
  };
};

const Icons = {
  settings: (c) => <svg width="24" height="24" viewBox="0 0 24 24" fill="none"><circle cx="12" cy="12" r="3" stroke={c} strokeWidth="1.5"/><path d="M12 3v2M12 19v2M3 12h2M19 12h2M5.64 5.64l1.41 1.41M16.95 16.95l1.41 1.41M5.64 18.36l1.41-1.41M16.95 7.05l1.41-1.41" stroke={c} strokeWidth="1.5" strokeLinecap="round"/></svg>,
  send: (c) => <svg width="24" height="24" viewBox="0 0 24 24" fill="none"><path d="M7 17L17.5 12L7 7l0 4l6 1l-6 1Z" fill={c}/></svg>,
  mic: (c) => <svg width="24" height="24" viewBox="0 0 24 24" fill="none"><rect x="9" y="3" width="6" height="11" rx="3" stroke={c} strokeWidth="1.5"/><path d="M5 12a7 7 0 0014 0M12 19v3" stroke={c} strokeWidth="1.5" strokeLinecap="round"/></svg>,
  check: (c) => <svg width="18" height="18" viewBox="0 0 18 18" fill="none"><path d="M4 9.5L7.5 13L14 5" stroke={c} strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"/></svg>,
  cal: (c) => <svg width="18" height="18" viewBox="0 0 18 18" fill="none"><rect x="2" y="3.5" width="14" height="12.5" rx="2" stroke={c} strokeWidth="1.5"/><path d="M2 7.5h14M6 1.5v3M12 1.5v3" stroke={c} strokeWidth="1.5" strokeLinecap="round"/></svg>,
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

const Orb = ({ color, size = 40 }) => (
  <div style={{ width: size, height: size, borderRadius: size / 2, background: color, display: "grid", placeItems: "center", flexShrink: 0 }}>
    <div style={{ width: size * 0.38, height: size * 0.38, borderRadius: size * 0.19, background: "rgba(0,0,0,0.15)" }} />
  </div>
);

const Bubble = ({ role, text, m }) => {
  const isUser = role === "user";
  if (role === "action") return (
    <div style={{ display: "flex", alignItems: "center", gap: g(1), padding: `${g(0.5)}px 0`, alignSelf: "flex-start" }}>
      {Icons.cal(m.onSurfaceVariant)}
      <span style={{ fontSize: 14, color: m.onSurfaceVariant, letterSpacing: 0.1 }}>{text}</span>
      {Icons.check(m.primary)}
    </div>
  );
  return (
    <div style={{ alignSelf: isUser ? "flex-end" : "flex-start", maxWidth: "82%" }}>
      <div style={{
        padding: `${g(1.5)}px ${g(2)}px`,
        borderRadius: isUser ? "20px 20px 4px 20px" : "20px 20px 20px 4px",
        /* M3: user gets primaryContainer, assistant gets surfaceHigh */
        background: isUser ? m.primaryContainer : m.surfaceHigh,
        color: isUser ? m.onPrimaryContainer : m.onSurface,
        fontSize: 16, lineHeight: "24px", letterSpacing: 0.15,
        whiteSpace: "pre-wrap",
      }}>{text}</div>
    </div>
  );
};

const Controls = ({ flavor, setFlavor, theme, setTheme, showEmpty, setShowEmpty }) => (
  <div style={{ position: "fixed", top: g(1), right: g(1), zIndex: 10 }}>
    <div style={{ background: "#1C1C1E", borderRadius: 12, padding: g(1.5), display: "flex", flexDirection: "column", gap: g(1.5), color: "#fff" }}>
      <div>
        <div style={{ fontSize: 11, fontWeight: 600, color: "rgba(235,235,245,0.30)", letterSpacing: 0.5, textTransform: "uppercase", marginBottom: g(1) }}>Theme</div>
        <div style={{ display: "flex", gap: g(0.75) }}>
          {["dark", "light"].map(th => (
            <button key={th} onClick={() => setTheme(th)} style={{
              padding: `${g(0.75)}px ${g(1.5)}px`, borderRadius: 8, background: theme === th ? "#3A3A3C" : "transparent",
              border: "none", fontSize: 12, fontWeight: 500, cursor: "pointer", color: theme === th ? "#fff" : "rgba(235,235,245,0.30)", textTransform: "capitalize",
            }}>{th}</button>
          ))}
        </div>
      </div>
      <div>
        <div style={{ fontSize: 11, fontWeight: 600, color: "rgba(235,235,245,0.30)", letterSpacing: 0.5, textTransform: "uppercase", marginBottom: g(1) }}>Flavor</div>
        <div style={{ display: "flex", gap: g(1), alignItems: "center" }}>
          {Object.entries(palettes).map(([n, p]) => (
            <button key={n} onClick={() => setFlavor(n)} style={{ width: 28, height: 28, borderRadius: 14, background: p.primary, cursor: "pointer", border: flavor === n ? `2px solid #fff` : "2px solid transparent" }} />
          ))}
        </div>
      </div>
      <div style={{ display: "flex", gap: g(0.75) }}>
        {[["Chat", false], ["Empty", true]].map(([label, val]) => (
          <button key={label} onClick={() => setShowEmpty(val)} style={{
            padding: `${g(0.75)}px ${g(1.5)}px`, borderRadius: 8, background: showEmpty === val ? "#3A3A3C" : "transparent",
            border: "none", fontSize: 12, fontWeight: 500, cursor: "pointer", color: showEmpty === val ? "#fff" : "rgba(235,235,245,0.30)",
          }}>{label}</button>
        ))}
      </div>
    </div>
  </div>
);

export default function MaterialYouChat() {
  const [flavor, setFlavor] = useState("tangerine");
  const [theme, setTheme] = useState("dark");
  const [inputValue, setInputValue] = useState("");
  const [showEmpty, setShowEmpty] = useState(false);
  const inputRef = useRef(null);
  const isDark = theme === "dark";
  const m = resolveM3(flavor, isDark);
  const hasText = inputValue.trim().length > 0;

  return (
    <div style={{
      fontFamily: "'Google Sans', 'Roboto Flex', Roboto, system-ui, sans-serif",
      background: m.bg, width: 393, minHeight: 852,
      display: "flex", flexDirection: "column", margin: "0 auto",
      WebkitFontSmoothing: "antialiased", transition: "background 300ms cubic-bezier(0.2, 0, 0, 1)",
    }}>
      <Controls flavor={flavor} setFlavor={setFlavor} theme={theme} setTheme={setTheme} showEmpty={showEmpty} setShowEmpty={setShowEmpty} />

      {/* Top App Bar — M3 medium (with flavor-tinted surface) */}
      <nav style={{
        display: "flex", alignItems: "center", padding: `${g(1.5)}px ${g(2)}px`,
        gap: g(2), height: 64, flexShrink: 0,
        background: m.surface,
      }}>
        <Orb color={m.primary} size={40} />
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ fontSize: 22, fontWeight: 400, letterSpacing: 0, color: m.onSurface, lineHeight: "28px" }}>Fawx</div>
          <div style={{ fontSize: 14, letterSpacing: 0.25, color: m.onSurfaceVariant, lineHeight: "20px" }}>Claude Sonnet 4.5</div>
        </div>
        <button style={{ width: 48, height: 48, display: "grid", placeItems: "center", background: "none", border: "none", cursor: "pointer", borderRadius: 24 }} aria-label="Settings">
          {Icons.settings(m.onSurfaceVariant)}
        </button>
      </nav>

      {/* Messages */}
      <div style={{ flex: 1, overflowY: "auto", padding: `${g(2)}px ${g(2)}px ${g(1)}px`, display: "flex", flexDirection: "column", gap: g(1) }}>
        {showEmpty ? (
          <div style={{ flex: 1, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: g(4), paddingBottom: g(12) }}>
            <Orb color={m.primary} size={64} />
            <div style={{ fontSize: 24, fontWeight: 400, letterSpacing: 0, color: m.onSurface }}>How can I help?</div>
            {/* M3 suggestion chips — rounded, outlined */}
            <div style={{ display: "flex", flexWrap: "wrap", gap: g(1), justifyContent: "center", padding: `0 ${g(3)}px` }}>
              {SUGGESTIONS.map((s, i) => (
                <button key={i} style={{
                  padding: `${g(1)}px ${g(2)}px`, borderRadius: 8,
                  background: "transparent",
                  border: `1px solid ${m.outline}`,
                  fontSize: 14, letterSpacing: 0.1, color: m.onSurfaceVariant,
                  cursor: "pointer", lineHeight: "20px",
                }}>{s}</button>
              ))}
            </div>
          </div>
        ) : (
          MESSAGES.map((msg, i) => <Bubble key={i} role={msg.role} text={msg.text} m={m} />)
        )}
      </div>

      {/* Input area — M3 style with FAB send */}
      <div style={{ padding: `${g(1)}px ${g(2)}px ${g(5)}px`, flexShrink: 0 }}>
        <div style={{ display: "flex", alignItems: "flex-end", gap: g(1.5) }}>
          <div style={{
            flex: 1, background: m.surfaceHighest, borderRadius: 28,
            display: "flex", alignItems: "flex-end", minHeight: 56,
            padding: `0 ${g(0.5)}px 0 ${g(3)}px`,
          }}>
            <input
              ref={inputRef} type="text" value={inputValue}
              onChange={e => setInputValue(e.target.value)}
              placeholder="Message"
              style={{
                flex: 1, background: "none", border: "none", outline: "none",
                fontSize: 16, letterSpacing: 0.15, lineHeight: "24px",
                color: m.onSurface, padding: `${g(2)}px 0`,
                caretColor: m.primary,
              }}
            />
            {!hasText && (
              <button style={{ width: 48, height: 48, display: "grid", placeItems: "center", background: "none", border: "none", cursor: "pointer", flexShrink: 0 }} aria-label="Voice">
                {Icons.mic(m.onSurfaceVariant)}
              </button>
            )}
          </div>
          {/* FAB send button — always primary */}
          <button style={{
            width: 56, height: 56, borderRadius: 16,
            background: hasText ? m.primary : m.surfaceHigh,
            border: "none", cursor: hasText ? "pointer" : "default",
            display: "grid", placeItems: "center", flexShrink: 0,
            transition: "background 200ms cubic-bezier(0.2, 0, 0, 1)",
            boxShadow: hasText ? "0 1px 3px rgba(0,0,0,0.3), 0 4px 8px rgba(0,0,0,0.15)" : "none",
          }} aria-label="Send">
            {Icons.send(hasText ? m.onPrimary : m.onSurfaceVariant)}
          </button>
        </div>
      </div>
    </div>
  );
}