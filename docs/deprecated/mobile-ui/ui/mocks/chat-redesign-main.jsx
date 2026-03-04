import { useState, useRef, useEffect } from "react";

const FLAVORS = {
  lemon: { primary: "#FFD600", glow: "#FFF9C4", tint: "#332B00", surface: "#1A1700" },
  tangerine: { primary: "#FF8C00", glow: "#FFE0B2", tint: "#331C00", surface: "#1A0E00" },
  lime: { primary: "#7CB342", glow: "#DCEDC8", tint: "#1A2E0D", surface: "#0D1706" },
  blood_orange: { primary: "#D84315", glow: "#FFCCBC", tint: "#2E0D04", surface: "#170602" },
  grapefruit: { primary: "#E91E63", glow: "#F8BBD0", tint: "#2E0413", surface: "#17020A" },
};

// --- iOS-style color tokens ---
const tokens = {
  bg: "#000000",
  surface: "#1C1C1E",
  surfaceElevated: "#2C2C2E",
  surfaceTertiary: "#3A3A3C",
  label: "#FFFFFF",
  labelSecondary: "rgba(235,235,245,0.6)",
  labelTertiary: "rgba(235,235,245,0.3)",
  separator: "rgba(84,84,88,0.65)",
  separatorLight: "rgba(84,84,88,0.35)",
};

const SegmentedControl = ({ options, value, onChange, style }) => (
  <div style={{
    display: "inline-flex", background: tokens.surfaceElevated,
    borderRadius: 9, padding: 2, gap: 1, ...style
  }}>
    {options.map(opt => (
      <button key={opt.value} onClick={() => onChange(opt.value)} style={{
        padding: "6px 14px", borderRadius: 7, border: "none", cursor: "pointer",
        fontSize: 13, fontWeight: 500, transition: "all 0.2s ease",
        background: value === opt.value ? tokens.surfaceTertiary : "transparent",
        color: value === opt.value ? tokens.label : tokens.labelSecondary,
      }}>{opt.label}</button>
    ))}
  </div>
);

// Chat bubble component
const MessageBubble = ({ message, flavor, design }) => {
  const f = FLAVORS[flavor];
  const isUser = message.role === "user";
  const isAction = message.role === "action";

  if (design === "current") {
    // Current: heavy glass morphism
    return (
      <div style={{
        alignSelf: isUser ? "flex-end" : "flex-start",
        maxWidth: "80%", marginBottom: 8,
      }}>
        <div style={{
          padding: "12px 16px",
          borderRadius: isUser ? "16px 16px 4px 16px" : "16px 16px 16px 4px",
          background: isUser
            ? `linear-gradient(135deg, ${f.tint}cc, ${f.tint}88)`
            : isAction
              ? `${tokens.surfaceElevated}80`
              : `${tokens.surface}dd`,
          border: isUser
            ? `1px solid ${f.primary}6b`
            : isAction
              ? `1px solid ${f.primary}57`
              : `1px solid ${tokens.separator}`,
          backdropFilter: "blur(20px)",
          boxShadow: isUser ? `0 0 20px ${f.primary}15` : "none",
          position: "relative",
          overflow: "hidden",
        }}>
          {/* Glass layer simulation */}
          <div style={{
            position: "absolute", inset: 0,
            background: `linear-gradient(180deg, rgba(255,255,255,0.06) 0%, transparent 40%)`,
            pointerEvents: "none",
          }} />
          <div style={{
            position: "absolute", bottom: 0, right: 0, width: "60%", height: "60%",
            background: `radial-gradient(ellipse at bottom right, ${f.primary}0d, transparent)`,
            pointerEvents: "none",
          }} />
          <span style={{
            fontSize: 15, lineHeight: 1.5, position: "relative",
            color: isUser ? tokens.label : tokens.labelSecondary,
          }}>{message.text}</span>
        </div>
        {message.label && (
          <span style={{ fontSize: 11, color: tokens.labelTertiary, marginTop: 2, display: "block", textAlign: isUser ? "right" : "left", paddingInline: 4 }}>
            {message.label}
          </span>
        )}
      </div>
    );
  }

  // Proposed: clean, iOS-like
  return (
    <div style={{
      alignSelf: isUser ? "flex-end" : "flex-start",
      maxWidth: "78%", marginBottom: 6,
    }}>
      <div style={{
        padding: "10px 14px",
        borderRadius: isUser ? "18px 18px 4px 18px" : "18px 18px 18px 4px",
        background: isUser ? f.primary : tokens.surfaceElevated,
        color: isUser ? (flavor === "lemon" ? "#1a1a00" : "#fff") : tokens.label,
        fontSize: 15, lineHeight: 1.48,
        letterSpacing: -0.2,
      }}>{message.text}</div>
      {message.label && (
        <span style={{ fontSize: 11, color: tokens.labelTertiary, marginTop: 3, display: "block", textAlign: isUser ? "right" : "left", paddingInline: 6 }}>
          {message.label}
        </span>
      )}
    </div>
  );
};

const MESSAGES = [
  { role: "user", text: "Set a reminder for my 3pm meeting with Sarah" },
  { role: "action", text: "Created reminder: \"Meeting with Sarah\" at 3:00 PM", label: "↗ calendar" },
  { role: "assistant", text: "Done — you'll get a notification 10 minutes before. Want me to also draft a quick agenda?" },
  { role: "user", text: "Yes, keep it short. Three bullet points max." },
  { role: "assistant", text: "Here's a quick agenda:\n\n• Q1 metrics review\n• Product roadmap priorities\n• Hiring timeline for engineering" },
];

const SUGGESTIONS = [
  "What's on my calendar today?",
  "Read my last 3 messages",
  "Set my phone to DND",
];

export default function FawxChatRedesign() {
  const [design, setDesign] = useState("proposed");
  const [flavor, setFlavor] = useState("tangerine");
  const [inputText, setInputText] = useState("");
  const [showEmpty, setShowEmpty] = useState(false);
  const f = FLAVORS[flavor];
  const messagesRef = useRef(null);

  const isCurrent = design === "current";

  return (
    <div style={{
      fontFamily: "-apple-system, SF Pro Display, SF Pro Text, system-ui, sans-serif",
      background: "#111", minHeight: "100vh", display: "flex", flexDirection: "column", alignItems: "center",
      padding: "24px 16px", color: tokens.label,
    }}>
      {/* Controls */}
      <div style={{ width: "100%", maxWidth: 420, marginBottom: 20 }}>
        <h2 style={{ fontSize: 22, fontWeight: 700, letterSpacing: -0.5, margin: 0, marginBottom: 16 }}>
          Chat Screen Comparison
        </h2>
        <div style={{ display: "flex", gap: 12, flexWrap: "wrap", alignItems: "center", marginBottom: 12 }}>
          <SegmentedControl
            options={[{ label: "Current", value: "current" }, { label: "Proposed", value: "proposed" }]}
            value={design} onChange={setDesign}
          />
          <SegmentedControl
            options={[{ label: "Messages", value: false }, { label: "Empty State", value: true }]}
            value={showEmpty} onChange={setShowEmpty}
          />
        </div>
        <div style={{ display: "flex", gap: 6, marginBottom: 4 }}>
          {Object.entries(FLAVORS).map(([name, fl]) => (
            <button key={name} onClick={() => setFlavor(name)} style={{
              width: 28, height: 28, borderRadius: 14, border: flavor === name ? `2px solid ${fl.primary}` : "2px solid transparent",
              background: fl.primary, cursor: "pointer", transition: "all 0.15s",
              transform: flavor === name ? "scale(1.15)" : "scale(1)",
            }} title={name} />
          ))}
        </div>
      </div>

      {/* Phone frame */}
      <div style={{
        width: 390, height: 844, borderRadius: 44, overflow: "hidden",
        border: "3px solid #333", background: tokens.bg, position: "relative",
        display: "flex", flexDirection: "column",
        boxShadow: "0 20px 60px rgba(0,0,0,0.5)",
      }}>
        {/* Status bar */}
        <div style={{
          height: 54, padding: "14px 24px 0", display: "flex", justifyContent: "space-between",
          alignItems: "center", fontSize: 14, fontWeight: 600, flexShrink: 0,
        }}>
          <span>9:41</span>
          <div style={{ display: "flex", gap: 5, alignItems: "center" }}>
            <div style={{ width: 17, height: 11, border: `1px solid ${tokens.label}`, borderRadius: 2, position: "relative" }}>
              <div style={{ position: "absolute", inset: 1.5, borderRadius: 0.5, background: tokens.label }} />
            </div>
          </div>
        </div>

        {/* Top bar */}
        {isCurrent ? (
          /* CURRENT: Heavy top bar with particles and glass */
          <div style={{
            padding: "8px 16px 12px", flexShrink: 0, position: "relative", overflow: "hidden",
          }}>
            {/* Simulated particle backdrop */}
            <div style={{ position: "absolute", inset: 0, overflow: "hidden" }}>
              {Array.from({ length: 20 }).map((_, i) => (
                <div key={i} style={{
                  position: "absolute",
                  left: `${(i * 37 + 13) % 100}%`,
                  top: `${(i * 23 + 7) % 100}%`,
                  width: 3 + (i % 4), height: 3 + (i % 4),
                  borderRadius: "50%",
                  background: `${f.primary}${i % 2 === 0 ? "30" : "18"}`,
                  animation: `float-${i % 3} 8s infinite ease-in-out`,
                }} />
              ))}
            </div>
            <div style={{ display: "flex", alignItems: "center", gap: 10, position: "relative" }}>
              {/* Glass icon container */}
              <div style={{
                width: 38, height: 38, borderRadius: 12,
                background: `linear-gradient(135deg, ${f.tint}99, ${f.tint}55)`,
                border: `1px solid ${f.primary}44`,
                display: "flex", alignItems: "center", justifyContent: "center",
                boxShadow: `0 0 16px ${f.primary}22`,
              }}>
                <div style={{
                  width: 22, height: 22, borderRadius: "50%",
                  background: `radial-gradient(circle, ${f.primary}, ${f.tint})`,
                  boxShadow: `0 0 8px ${f.primary}55`,
                }} />
              </div>
              {/* Model chip with glass */}
              <div style={{
                padding: "5px 12px", borderRadius: 14,
                background: `linear-gradient(135deg, ${tokens.surfaceElevated}cc, ${tokens.surface}88)`,
                border: `1px solid ${f.primary}33`,
                backdropFilter: "blur(10px)",
                fontSize: 13, color: tokens.labelSecondary,
              }}>
                Claude Sonnet ▾
              </div>
              <div style={{ flex: 1 }} />
              {/* Toolbar */}
              <div style={{ display: "flex", gap: 8 }}>
                {["⚙", "◫", "🗑"].map((icon, i) => (
                  <div key={i} style={{
                    width: 32, height: 32, borderRadius: 10,
                    background: `${tokens.surfaceElevated}88`,
                    border: `1px solid ${tokens.separator}`,
                    display: "flex", alignItems: "center", justifyContent: "center",
                    fontSize: 14, cursor: "pointer",
                  }}>{icon}</div>
                ))}
              </div>
            </div>
          </div>
        ) : (
          /* PROPOSED: Clean iOS-style nav bar */
          <div style={{
            padding: "4px 16px 10px", flexShrink: 0,
            display: "flex", alignItems: "center", gap: 10,
            borderBottom: `0.5px solid ${tokens.separatorLight}`,
          }}>
            <div style={{
              width: 32, height: 32, borderRadius: "50%",
              background: f.primary,
              display: "flex", alignItems: "center", justifyContent: "center",
            }}>
              <div style={{ width: 16, height: 16, borderRadius: "50%", background: "rgba(0,0,0,0.2)" }} />
            </div>
            <div style={{ flex: 1 }}>
              <div style={{ fontSize: 16, fontWeight: 600, letterSpacing: -0.3 }}>Fawx</div>
              <div style={{ fontSize: 12, color: tokens.labelTertiary, marginTop: -1 }}>Claude Sonnet</div>
            </div>
            <button style={{
              background: "none", border: "none", color: tokens.labelSecondary,
              fontSize: 13, cursor: "pointer", padding: "6px 10px",
              borderRadius: 16, background: tokens.surfaceElevated,
            }}>
              ⚙ Settings
            </button>
          </div>
        )}

        {/* Messages area */}
        <div ref={messagesRef} style={{
          flex: 1, overflowY: "auto", padding: "16px 12px",
          display: "flex", flexDirection: "column",
          gap: isCurrent ? 4 : 2,
        }}>
          {showEmpty ? (
            /* Empty state */
            isCurrent ? (
              <div style={{
                flex: 1, display: "flex", flexDirection: "column",
                alignItems: "center", justifyContent: "center", gap: 16, padding: 20,
              }}>
                {/* Big animated orb */}
                <div style={{
                  width: 80, height: 80, borderRadius: "50%", position: "relative",
                  background: `radial-gradient(circle at 35% 35%, ${f.glow}, ${f.primary}, ${f.tint})`,
                  boxShadow: `0 0 40px ${f.primary}44, 0 0 80px ${f.primary}22`,
                }}>
                  {[...Array(3)].map((_, i) => (
                    <div key={i} style={{
                      position: "absolute", inset: -8 - i * 6, borderRadius: "50%",
                      border: `1px solid ${f.primary}${15 - i * 4}`,
                    }} />
                  ))}
                </div>
                <p style={{ fontSize: 16, color: tokens.labelSecondary, textAlign: "center" }}>
                  What can I help with?
                </p>
                <div style={{ display: "flex", flexDirection: "column", gap: 8, width: "100%" }}>
                  {SUGGESTIONS.map((s, i) => (
                    <div key={i} style={{
                      padding: "12px 16px", borderRadius: 14,
                      background: `linear-gradient(135deg, ${tokens.surfaceElevated}cc, ${tokens.surface}88)`,
                      border: `1px solid ${f.primary}25`,
                      backdropFilter: "blur(10px)",
                      fontSize: 14, color: tokens.labelSecondary, cursor: "pointer",
                      boxShadow: `0 0 12px ${f.primary}08`,
                    }}>{s}</div>
                  ))}
                </div>
              </div>
            ) : (
              <div style={{
                flex: 1, display: "flex", flexDirection: "column",
                alignItems: "center", justifyContent: "center", gap: 20, padding: 24,
              }}>
                <div style={{
                  width: 56, height: 56, borderRadius: "50%",
                  background: f.primary, opacity: 0.9,
                  display: "flex", alignItems: "center", justifyContent: "center",
                }}>
                  <div style={{ width: 26, height: 26, borderRadius: "50%", background: "rgba(0,0,0,0.15)" }} />
                </div>
                <p style={{ fontSize: 20, fontWeight: 600, letterSpacing: -0.5, color: tokens.label, margin: 0 }}>
                  How can I help?
                </p>
                <div style={{ display: "flex", flexWrap: "wrap", gap: 8, justifyContent: "center" }}>
                  {SUGGESTIONS.map((s, i) => (
                    <button key={i} style={{
                      padding: "10px 16px", borderRadius: 20,
                      background: tokens.surfaceElevated,
                      border: "none",
                      fontSize: 14, color: tokens.labelSecondary, cursor: "pointer",
                      transition: "background 0.15s",
                    }}>{s}</button>
                  ))}
                </div>
              </div>
            )
          ) : (
            MESSAGES.map((msg, i) => (
              <MessageBubble key={i} message={msg} flavor={flavor} design={design} />
            ))
          )}
        </div>

        {/* Input bar */}
        {isCurrent ? (
          <div style={{
            padding: "10px 12px 34px", flexShrink: 0,
            borderTop: `1px solid ${tokens.separator}`,
          }}>
            <div style={{
              display: "flex", gap: 8, alignItems: "flex-end",
            }}>
              <div style={{
                flex: 1, padding: "10px 14px", borderRadius: 20,
                background: `linear-gradient(135deg, ${tokens.surfaceElevated}cc, ${tokens.surface}88)`,
                border: `1px solid ${tokens.separator}`,
                backdropFilter: "blur(10px)",
                fontSize: 15, color: tokens.labelTertiary,
                minHeight: 20,
              }}>
                Ask anything...
              </div>
              <div style={{
                width: 36, height: 36, borderRadius: 18,
                background: `linear-gradient(135deg, ${f.primary}, ${f.tint})`,
                border: `1px solid ${f.primary}66`,
                display: "flex", alignItems: "center", justifyContent: "center",
                fontSize: 16, cursor: "pointer",
                boxShadow: `0 0 12px ${f.primary}33`,
              }}>🎤</div>
              <div style={{
                width: 36, height: 36, borderRadius: 18,
                background: `${tokens.surfaceElevated}88`,
                border: `1px solid ${tokens.separator}`,
                display: "flex", alignItems: "center", justifyContent: "center",
                fontSize: 16, cursor: "pointer",
              }}>↑</div>
            </div>
          </div>
        ) : (
          <div style={{
            padding: "8px 12px 34px", flexShrink: 0,
            borderTop: `0.5px solid ${tokens.separatorLight}`,
            background: `${tokens.bg}ee`,
            backdropFilter: "blur(20px)",
          }}>
            <div style={{
              display: "flex", gap: 8, alignItems: "flex-end",
            }}>
              <div style={{
                flex: 1, padding: "10px 16px", borderRadius: 22,
                background: tokens.surfaceElevated,
                fontSize: 15, color: tokens.labelTertiary,
                minHeight: 20,
              }}>
                Message
              </div>
              <div style={{
                width: 36, height: 36, borderRadius: "50%",
                background: f.primary,
                display: "flex", alignItems: "center", justifyContent: "center",
                cursor: "pointer",
              }}>
                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke={flavor === "lemon" ? "#1a1a00" : "#fff"} strokeWidth="2.5" strokeLinecap="round">
                  <line x1="12" y1="19" x2="12" y2="5" />
                  <polyline points="5 12 12 5 19 12" />
                </svg>
              </div>
            </div>
          </div>
        )}
      </div>

      {/* Design notes */}
      <div style={{
        maxWidth: 420, width: "100%", marginTop: 24, padding: 20,
        background: tokens.surface, borderRadius: 16, fontSize: 13,
        color: tokens.labelSecondary, lineHeight: 1.6,
      }}>
        <div style={{ fontWeight: 600, color: tokens.label, marginBottom: 8, fontSize: 15 }}>
          {isCurrent ? "Current Design Notes" : "Proposed Changes"}
        </div>
        {isCurrent ? (
          <div>
            <p style={{ margin: "0 0 8px" }}>The current design uses multi-layer glass morphism with particle effects, warm glow borders, and radial gradient accents on every surface. While distinctive, it creates visual density that competes with content.</p>
            <p style={{ margin: 0 }}>Issues: simulated 3D orbs with 56 particles in the top bar, glass gradients on every bubble, glow shadows on buttons, and backdrop blur on inputs all add rendering cost and visual noise.</p>
          </div>
        ) : (
          <ul style={{ margin: 0, paddingLeft: 16 }}>
            <li style={{ marginBottom: 6 }}>Flat, opaque surfaces — no glass layers, no glow shadows</li>
            <li style={{ marginBottom: 6 }}>User bubbles use solid flavor color (like iMessage)</li>
            <li style={{ marginBottom: 6 }}>iOS-style nav: compact title + subtitle, single settings button</li>
            <li style={{ marginBottom: 6 }}>Pill-shaped suggestion chips instead of card-style</li>
            <li style={{ marginBottom: 6 }}>Single send button (voice can be a long-press)</li>
            <li>Tighter spacing, SF Pro metrics, -0.2 letter-spacing</li>
          </ul>
        )}
      </div>
    </div>
  );
}