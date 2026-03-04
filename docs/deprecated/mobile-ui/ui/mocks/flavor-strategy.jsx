import { useState } from "react";

/*
  Three flavor strategies compared side-by-side.
  Same conversation, same layout, different color application rules.
  4pt spatial grid. No phone chrome. No emoji.
  Supports dark/light theme + "no flavor" option.
*/

const GRID = 4;
const g = (n) => n * GRID;

/* ── Theme tokens (iOS system colors) ── */
const themes = {
  dark: {
    bg:              "#000000",
    surface1:        "#1C1C1E",
    surface2:        "#2C2C2E",
    surface3:        "#3A3A3C",
    surface4:        "#48484A",
    labelPrimary:    "#FFFFFF",
    labelSecondary:  "rgba(235,235,245,0.60)",
    labelTertiary:   "rgba(235,235,245,0.30)",
    labelQuaternary: "rgba(235,235,245,0.18)",
    separator:       "rgba(84,84,88,0.36)",
    green:           "#30D158",
    noFlavorOrb:     "#FFFFFF",
    noFlavorOrbInner:"rgba(0,0,0,0.12)",
    noFlavorBubble:  "#48484A",
    noFlavorSend:    "#3A3A3C",
    noFlavorSendIcon:"rgba(235,235,245,0.60)",
  },
  light: {
    bg:              "#FFFFFF",
    surface1:        "#F2F2F7",
    surface2:        "#E5E5EA",
    surface3:        "#D1D1D6",
    surface4:        "#C7C7CC",
    labelPrimary:    "#000000",
    labelSecondary:  "rgba(60,60,67,0.60)",
    labelTertiary:   "rgba(60,60,67,0.30)",
    labelQuaternary: "rgba(60,60,67,0.18)",
    separator:       "rgba(60,60,67,0.12)",
    green:           "#34C759",
    noFlavorOrb:     "#000000",
    noFlavorOrbInner:"rgba(255,255,255,0.20)",
    noFlavorBubble:  "#C7C7CC",
    noFlavorSend:    "#D1D1D6",
    noFlavorSendIcon:"rgba(60,60,67,0.60)",
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

/* ── Icons ── */
const SendIcon = (color) => (
  <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
    <path d="M7 12V2M3 6l4-4 4 4" stroke={color} strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"/>
  </svg>
);
const CalIcon = (color) => (
  <svg width="13" height="13" viewBox="0 0 13 13" fill="none">
    <rect x="1.25" y="2.25" width="10.5" height="9.5" rx="1.5" stroke={color} strokeWidth="1.1"/>
    <path d="M1.25 5.25h10.5M4 1v2.5M9 1v2.5" stroke={color} strokeWidth="1.1" strokeLinecap="round"/>
  </svg>
);
const CheckIcon = (color) => (
  <svg width="10" height="10" viewBox="0 0 10 10" fill="none">
    <path d="M2 5.5L4.2 7.5 8 3" stroke={color} strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round"/>
  </svg>
);
const GearIcon = (color) => (
  <svg width="18" height="18" viewBox="0 0 18 18" fill="none">
    <circle cx="9" cy="9" r="2.2" stroke={color} strokeWidth="1.3"/>
    <path d="M9 2.5v1.2M9 14.3v1.2M2.5 9h1.2M14.3 9h1.2M4.2 4.2l.85.85M12.95 12.95l.85.85M4.2 13.8l.85-.85M12.95 5.05l.85-.85" stroke={color} strokeWidth="1.3" strokeLinecap="round"/>
  </svg>
);

/* ── Orb ── */
const Orb = ({ color, innerColor = "rgba(0,0,0,0.12)", size }) => (
  <div style={{
    width: size, height: size, borderRadius: size / 2,
    background: color, display: "grid", placeItems: "center", flexShrink: 0,
  }}>
    <div style={{
      width: Math.round(size * 0.38), height: Math.round(size * 0.38),
      borderRadius: Math.round(size * 0.19),
      background: innerColor,
    }} />
  </div>
);

/* ── Strategy definitions ── */
const strategies = {
  everywhere: {
    label: "A. Flavor Everywhere",
    desc: "Accent color on bubbles, send button, orb, action icons, suggestion chip borders, thinking dot, nav subtitle, and input caret.",
  },
  orbOnly: {
    label: "B. Orb Only",
    desc: "Flavor color applies to the orb and nothing else. All interactive elements use system neutral colors.",
  },
  threePoint: {
    label: "C. Three Touch-points",
    desc: "Flavor on the orb, user message bubbles, and send button. Everything else is system neutral.",
  },
};

const MESSAGES = [
  { role: "user", text: "Set a reminder for my 3pm meeting" },
  { role: "action", text: "Reminder created — 3:00 PM" },
  { role: "assistant", text: "Done. You'll get a notification 10 minutes before." },
  { role: "user", text: "Also put my phone on DND" },
  { role: "action", text: "Do Not Disturb enabled" },
  { role: "assistant", text: "DND is on. I'll turn it off after your meeting ends at 4." },
];

/* ── Resolve colors for a given strategy + flavor + theme ── */
const resolveStrategy = (strategy, flavorKey, t) => {
  const f = FLAVORS[flavorKey];
  const noFlavor = flavorKey === "none";
  const s = strategy;

  // Orb: always shows flavor (or neutral for "none")
  const orbColor = noFlavor ? t.noFlavorOrb : f.primary;
  const orbInner = noFlavor ? t.noFlavorOrbInner : "rgba(0,0,0,0.12)";

  // For "none", everything goes neutral regardless of strategy
  if (noFlavor) {
    return {
      orbColor, orbInner,
      userBubbleBg:   t.noFlavorBubble,
      userBubbleText: t.labelPrimary,
      sendBg:         t.noFlavorSend,
      sendIcon:       t.noFlavorSendIcon,
      actionIconClr:  t.labelTertiary,
      chipBorder:     `1px solid ${t.separator}`,
      chipText:       t.labelSecondary,
      subtitleColor:  t.labelTertiary,
      thinkingDot:    t.labelTertiary,
    };
  }

  return {
    orbColor, orbInner,
    userBubbleBg:   s === "orbOnly" ? t.surface3           : f.primary,
    userBubbleText: s === "orbOnly" ? t.labelPrimary       : f.onPrimary,
    sendBg:         s === "orbOnly" ? t.surface3            : f.primary,
    sendIcon:       s === "orbOnly" ? t.labelSecondary      : f.onPrimary,
    actionIconClr:  s === "everywhere" ? f.primary          : t.labelTertiary,
    chipBorder:     s === "everywhere" ? `1px solid ${f.primary}33` : `1px solid ${t.separator}`,
    chipText:       s === "everywhere" ? f.primary          : t.labelSecondary,
    subtitleColor:  s === "everywhere" ? `${f.primary}99`   : t.labelTertiary,
    thinkingDot:    s === "everywhere" ? f.primary          : t.labelTertiary,
  };
};

/* ── Single chat column ── */
const ChatColumn = ({ strategy, flavor, t }) => {
  const r = resolveStrategy(strategy, flavor, t);

  return (
    <div style={{
      width: 320, background: t.bg, borderRadius: g(4),
      overflow: "hidden", display: "flex", flexDirection: "column",
      border: `1px solid ${t.separator}`,
      height: 620, flexShrink: 0,
      transition: "background 200ms ease",
    }}>
      {/* Nav */}
      <nav style={{
        display: "flex", alignItems: "center",
        padding: `${g(2.5)}px ${g(3.5)}px`,
        gap: g(2.5),
        borderBottom: `0.5px solid ${t.separator}`,
        flexShrink: 0,
      }}>
        <Orb color={r.orbColor} innerColor={r.orbInner} size={g(7)} />
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{
            fontSize: 16, fontWeight: 600, letterSpacing: -0.3,
            color: t.labelPrimary, lineHeight: "20px",
          }}>Fawx</div>
          <div style={{
            fontSize: 12, letterSpacing: -0.1,
            color: r.subtitleColor, lineHeight: "16px",
          }}>Claude Sonnet 4.5</div>
        </div>
        <div style={{
          width: g(9), height: g(9), display: "grid", placeItems: "center",
          cursor: "pointer", borderRadius: g(2.5),
        }}>
          {GearIcon(t.labelTertiary)}
        </div>
      </nav>

      {/* Messages */}
      <div style={{
        flex: 1, overflowY: "auto",
        padding: `${g(3)}px ${g(3)}px ${g(2)}px`,
        display: "flex", flexDirection: "column", gap: g(1.5),
      }}>
        {MESSAGES.map((msg, i) => {
          if (msg.role === "action") {
            return (
              <div key={i} style={{
                display: "flex", alignItems: "center", gap: g(1.5),
                padding: `${g(0.5)}px 0`, alignSelf: "flex-start",
              }}>
                {CalIcon(r.actionIconClr)}
                <span style={{ fontSize: 12.5, color: t.labelSecondary, letterSpacing: -0.1 }}>
                  {msg.text}
                </span>
                {CheckIcon(t.green)}
              </div>
            );
          }
          const isUser = msg.role === "user";
          return (
            <div key={i} style={{
              alignSelf: isUser ? "flex-end" : "flex-start",
              maxWidth: "84%",
            }}>
              <div style={{
                padding: `${g(2.25)}px ${g(3)}px`,
                borderRadius: isUser ? "16px 16px 4px 16px" : "16px 16px 16px 4px",
                background: isUser ? r.userBubbleBg : t.surface2,
                color: isUser ? r.userBubbleText : t.labelPrimary,
                fontSize: 15, lineHeight: "21px", letterSpacing: -0.2,
              }}>{msg.text}</div>
            </div>
          );
        })}

        {/* Thinking indicator */}
        <div style={{
          display: "flex", alignItems: "center", gap: g(1.5),
          padding: `${g(1)}px 0`, alignSelf: "flex-start",
        }}>
          <div style={{
            width: 5, height: 5, borderRadius: 2.5,
            background: r.thinkingDot, opacity: 0.5,
          }} />
          <span style={{ fontSize: 12.5, color: t.labelTertiary, fontStyle: "italic", letterSpacing: -0.1 }}>
            Thinking…
          </span>
        </div>
      </div>

      {/* Suggestion chips */}
      <div style={{
        padding: `0 ${g(3)}px ${g(2)}px`,
        display: "flex", gap: g(1.5), overflowX: "auto",
        flexShrink: 0,
      }}>
        {["Calendar today", "Read messages"].map((s, i) => (
          <button key={i} style={{
            padding: `${g(1.5)}px ${g(3)}px`,
            borderRadius: g(4),
            background: "transparent",
            border: r.chipBorder,
            fontSize: 12.5, letterSpacing: -0.1,
            color: r.chipText,
            cursor: "pointer", whiteSpace: "nowrap",
            flexShrink: 0,
          }}>{s}</button>
        ))}
      </div>

      {/* Input */}
      <div style={{
        padding: `${g(2)}px ${g(3)}px ${g(3)}px`,
        borderTop: `0.5px solid ${t.separator}`,
        flexShrink: 0,
      }}>
        <div style={{
          display: "flex", alignItems: "center", gap: g(2),
        }}>
          <div style={{
            flex: 1, background: t.surface2, borderRadius: g(5.5),
            padding: `${g(2)}px ${g(3.5)}px`,
            fontSize: 15, color: t.labelTertiary, letterSpacing: -0.2,
            lineHeight: "20px",
          }}>Message</div>
          <div style={{
            width: g(8), height: g(8), borderRadius: g(4),
            background: r.sendBg,
            display: "grid", placeItems: "center", flexShrink: 0,
          }}>
            {SendIcon(r.sendIcon)}
          </div>
        </div>
      </div>
    </div>
  );
};

/* ── Main ── */
export default function FlavorStrategy() {
  const [flavor, setFlavor] = useState("tangerine");
  const [theme, setTheme] = useState("dark");
  const [hovered, setHovered] = useState(null);

  const t = themes[theme];
  const ct = themes.dark; // control panel always dark

  return (
    <div style={{
      fontFamily: "-apple-system, 'SF Pro Text', 'SF Pro Display', system-ui, sans-serif",
      background: "#111", minHeight: "100vh",
      padding: `${g(8)}px ${g(6)}px`,
      WebkitFontSmoothing: "antialiased",
      color: ct.labelPrimary,
    }}>
      <div style={{ maxWidth: 1080, margin: "0 auto" }}>
        <h1 style={{
          fontSize: 24, fontWeight: 700, letterSpacing: -0.6,
          margin: 0,
        }}>Flavor Strategy Comparison</h1>
        <p style={{
          fontSize: 15, color: ct.labelSecondary, letterSpacing: -0.2,
          margin: `${g(1.5)}px 0 ${g(5)}px`, lineHeight: "22px",
        }}>
          Same layout, same content. The only difference is where the flavor color appears.
        </p>

        {/* Controls row */}
        <div style={{
          display: "flex", gap: g(6), alignItems: "center",
          marginBottom: g(6), flexWrap: "wrap",
        }}>
          {/* Theme */}
          <div>
            <div style={{ fontSize: 11, fontWeight: 600, color: ct.labelTertiary, letterSpacing: 0.3, textTransform: "uppercase", marginBottom: g(2) }}>
              Theme
            </div>
            <div style={{ display: "flex", gap: g(1.5) }}>
              {["dark", "light"].map(th => (
                <button key={th} onClick={() => setTheme(th)} style={{
                  padding: `${g(1.5)}px ${g(3)}px`, borderRadius: g(2),
                  background: theme === th ? ct.surface3 : "transparent",
                  border: "none", fontSize: 12, fontWeight: 500, cursor: "pointer",
                  color: theme === th ? ct.labelPrimary : ct.labelTertiary,
                  textTransform: "capitalize",
                }}>{th}</button>
              ))}
            </div>
          </div>

          {/* Flavor */}
          <div>
            <div style={{ fontSize: 11, fontWeight: 600, color: ct.labelTertiary, letterSpacing: 0.3, textTransform: "uppercase", marginBottom: g(2) }}>
              Flavor
            </div>
            <div style={{ display: "flex", gap: g(2), alignItems: "center" }}>
              {/* No flavor */}
              <button onClick={() => setFlavor("none")} style={{
                width: g(6), height: g(6), borderRadius: g(3),
                cursor: "pointer",
                background: "linear-gradient(135deg, #fff 50%, #000 50%)",
                border: flavor === "none" ? `2px solid ${ct.labelPrimary}` : "2px solid transparent",
                transition: "border-color 100ms ease",
              }} aria-label="None" title="None" />
              {/* Color flavors */}
              {Object.entries(FLAVORS).filter(([k]) => k !== "none").map(([name, fl]) => (
                <button key={name} onClick={() => setFlavor(name)} style={{
                  width: g(6), height: g(6), borderRadius: g(3),
                  background: fl.primary, cursor: "pointer",
                  border: flavor === name ? `2px solid ${ct.labelPrimary}` : "2px solid transparent",
                  outline: flavor === name ? `2px solid ${fl.primary}` : "none",
                  outlineOffset: 1,
                  transition: "all 100ms ease",
                }} aria-label={fl.label} title={fl.label} />
              ))}
            </div>
          </div>
        </div>

        {/* Strategy columns */}
        <div style={{
          display: "flex", gap: g(5),
          overflowX: "auto",
          paddingBottom: g(4),
        }}>
          {Object.entries(strategies).map(([key, strat]) => (
            <div
              key={key}
              style={{ display: "flex", flexDirection: "column", gap: g(3), flexShrink: 0 }}
              onMouseEnter={() => setHovered(key)}
              onMouseLeave={() => setHovered(null)}
            >
              <div>
                <div style={{
                  fontSize: 14, fontWeight: 600, letterSpacing: -0.2,
                  color: ct.labelPrimary, marginBottom: g(1),
                }}>{strat.label}</div>
                <div style={{
                  fontSize: 13, color: ct.labelTertiary, letterSpacing: -0.1,
                  lineHeight: "18px", maxWidth: 300,
                }}>{strat.desc}</div>
              </div>

              <div style={{
                transition: "transform 150ms ease, box-shadow 150ms ease",
                transform: hovered === key ? "translateY(-2px)" : "none",
                boxShadow: hovered === key ? "0 8px 32px rgba(0,0,0,0.4)" : "0 2px 8px rgba(0,0,0,0.2)",
                borderRadius: g(4),
              }}>
                <ChatColumn strategy={key} flavor={flavor} t={t} />
              </div>

              <div style={{
                fontSize: 12, color: ct.labelTertiary, letterSpacing: -0.1,
                lineHeight: "17px", maxWidth: 300,
              }}>
                {key === "everywhere" && (
                  <>
                    <strong style={{ color: ct.labelSecondary }}>Flavor touches:</strong>{" "}
                    {flavor === "none"
                      ? "With no flavor selected, this strategy collapses to neutral. The orb is the only distinction."
                      : "orb, user bubbles, send button, action icons, suggestion chip borders, thinking dot, nav subtitle, input caret"}
                  </>
                )}
                {key === "orbOnly" && (
                  <>
                    <strong style={{ color: ct.labelSecondary }}>Flavor touches:</strong>{" "}
                    {flavor === "none"
                      ? "Fully monochrome. White orb on dark, black orb on light. Pure tool aesthetic."
                      : "orb only. User bubbles fall back to surface3, send button to surface3. Monochrome with one spot of color."}
                  </>
                )}
                {key === "threePoint" && (
                  <>
                    <strong style={{ color: ct.labelSecondary }}>Flavor touches:</strong>{" "}
                    {flavor === "none"
                      ? "Neutral everywhere. Identical to orb-only since no flavor propagates to bubbles or send."
                      : "orb, user bubbles, send button. Action icons, chips, subtitle, and thinking dot stay neutral."}
                  </>
                )}
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}