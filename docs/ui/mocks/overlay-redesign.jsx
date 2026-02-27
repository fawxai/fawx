import { useState } from "react";

const FLAVORS = {
  lemon: { primary: "#FFD600", dark: "#1a1a00" },
  tangerine: { primary: "#FF8C00", dark: "#fff" },
  lime: { primary: "#7CB342", dark: "#fff" },
  blood_orange: { primary: "#D84315", dark: "#fff" },
  grapefruit: { primary: "#E91E63", dark: "#fff" },
};

const t = {
  bg: "#000000",
  surface: "#1C1C1E",
  elevated: "#2C2C2E",
  tertiary: "#3A3A3C",
  label: "#FFFFFF",
  secondary: "rgba(235,235,245,0.6)",
  labelTertiary: "rgba(235,235,245,0.3)",
  separator: "rgba(84,84,88,0.65)",
  separatorLight: "rgba(84,84,88,0.35)",
  green: "#30D158",
  red: "#FF453A",
};

// Simulated phone "desktop" background
const PhoneDesktop = ({ children, flavor }) => {
  const f = FLAVORS[flavor];
  return (
    <div style={{
      width: 390, height: 600, borderRadius: 44, overflow: "hidden",
      border: "3px solid #333", position: "relative",
      background: `linear-gradient(160deg, #1a1a2e 0%, #16213e 40%, #0f3460 100%)`,
      boxShadow: "0 20px 60px rgba(0,0,0,0.5)",
    }}>
      {/* Status bar */}
      <div style={{
        height: 54, padding: "14px 24px 0", display: "flex", justifyContent: "space-between",
        alignItems: "center", fontSize: 14, fontWeight: 600, color: "#fff", flexShrink: 0,
      }}>
        <span>9:41</span>
        <div style={{ width: 17, height: 11, border: "1px solid #fff", borderRadius: 2, position: "relative" }}>
          <div style={{ position: "absolute", inset: 1.5, borderRadius: 0.5, background: "#fff" }} />
        </div>
      </div>

      {/* Fake app grid */}
      <div style={{
        display: "grid", gridTemplateColumns: "repeat(4, 1fr)", gap: 20,
        padding: "40px 30px", opacity: 0.3,
      }}>
        {Array.from({ length: 8 }).map((_, i) => (
          <div key={i} style={{
            width: 56, height: 56, borderRadius: 14,
            background: ["#FF3B30", "#FF9500", "#FFCC00", "#34C759", "#007AFF", "#5856D6", "#AF52DE", "#FF2D55"][i],
          }} />
        ))}
      </div>

      {children}
    </div>
  );
};

export default function OverlayRedesign() {
  const [view, setView] = useState("proposed");
  const [flavor, setFlavor] = useState("tangerine");
  const [overlayMode, setOverlayMode] = useState("bubble"); // "bubble", "mini", "dynamic-island"
  const f = FLAVORS[flavor];

  return (
    <div style={{
      fontFamily: "-apple-system, SF Pro Display, SF Pro Text, system-ui, sans-serif",
      background: "#111", minHeight: "100vh", display: "flex", flexDirection: "column",
      alignItems: "center", padding: "24px 16px", color: t.label,
    }}>
      <div style={{ width: "100%", maxWidth: 800, marginBottom: 20 }}>
        <h2 style={{ fontSize: 22, fontWeight: 700, letterSpacing: -0.5, margin: "0 0 4px" }}>
          Overlay Modes Comparison
        </h2>
        <p style={{ fontSize: 14, color: t.secondary, margin: "0 0 16px" }}>
          How Fawx appears over other apps — current vs proposed alternatives
        </p>

        <div style={{ display: "flex", gap: 12, flexWrap: "wrap", alignItems: "center", marginBottom: 12 }}>
          <div style={{
            display: "inline-flex", background: t.elevated, borderRadius: 9, padding: 2,
          }}>
            {["current", "proposed"].map(v => (
              <button key={v} onClick={() => setView(v)} style={{
                padding: "6px 14px", borderRadius: 7, border: "none", cursor: "pointer",
                fontSize: 13, fontWeight: 500, transition: "all 0.2s",
                background: view === v ? t.tertiary : "transparent",
                color: view === v ? t.label : t.secondary, textTransform: "capitalize",
              }}>{v}</button>
            ))}
          </div>
          <div style={{
            display: "inline-flex", background: t.elevated, borderRadius: 9, padding: 2,
          }}>
            {["bubble", "mini", "dynamic-island"].map(m => (
              <button key={m} onClick={() => setOverlayMode(m)} style={{
                padding: "6px 12px", borderRadius: 7, border: "none", cursor: "pointer",
                fontSize: 12, fontWeight: 500, transition: "all 0.2s",
                background: overlayMode === m ? t.tertiary : "transparent",
                color: overlayMode === m ? t.label : t.secondary, textTransform: "capitalize",
              }}>{m.replace("-", " ")}</button>
            ))}
          </div>
          <div style={{ display: "flex", gap: 6 }}>
            {Object.entries(FLAVORS).map(([name, fl]) => (
              <button key={name} onClick={() => setFlavor(name)} style={{
                width: 22, height: 22, borderRadius: 11, cursor: "pointer",
                background: fl.primary, border: flavor === name ? "2px solid #fff" : "2px solid transparent",
              }} />
            ))}
          </div>
        </div>
      </div>

      <div style={{ display: "flex", gap: 24, flexWrap: "wrap", justifyContent: "center" }}>
        {/* Current overlays */}
        {view === "current" ? (
          <>
            <div>
              <div style={{ fontSize: 12, color: t.labelTertiary, marginBottom: 8, fontWeight: 600, textAlign: "center" }}>
                CURRENT · {overlayMode === "bubble" ? "Bubble" : "Mini Chat"}
              </div>
              <PhoneDesktop flavor={flavor}>
                {overlayMode === "bubble" ? (
                  /* Current bubble */
                  <div style={{
                    position: "absolute", bottom: 100, right: 20,
                  }}>
                    <div style={{
                      width: 56, height: 56, borderRadius: "50%", position: "relative",
                      background: `radial-gradient(circle at 35% 35%, ${f.primary}ee, ${f.primary}88, #111)`,
                      boxShadow: `0 0 20px ${f.primary}33, 0 4px 12px rgba(0,0,0,0.4)`,
                    }}>
                      {/* Progress ring */}
                      <svg style={{ position: "absolute", inset: -3 }} width="62" height="62" viewBox="0 0 62 62">
                        <circle cx="31" cy="31" r="28" fill="none" stroke={`${f.primary}44`} strokeWidth="2" />
                        <circle cx="31" cy="31" r="28" fill="none" stroke={f.primary} strokeWidth="2"
                          strokeDasharray="176" strokeDashoffset="44" strokeLinecap="round"
                          transform="rotate(-90 31 31)" />
                      </svg>
                      {/* Badge */}
                      <div style={{
                        position: "absolute", top: -4, right: -4,
                        width: 20, height: 20, borderRadius: "50%",
                        background: t.green, display: "flex", alignItems: "center", justifyContent: "center",
                        fontSize: 11, fontWeight: 700, color: "#fff",
                        border: "2px solid #1a1a2e",
                      }}>✓</div>
                    </div>
                  </div>
                ) : (
                  /* Current mini chat */
                  <div style={{
                    position: "absolute", bottom: 0, left: 0, right: 0,
                    height: "42%", borderRadius: "20px 20px 0 0",
                    background: `${t.surface}ee`,
                    backdropFilter: "blur(20px)",
                    border: `1px solid ${t.separator}`,
                    borderBottom: "none",
                    display: "flex", flexDirection: "column",
                    overflow: "hidden",
                  }}>
                    {/* Header */}
                    <div style={{
                      display: "flex", alignItems: "center", gap: 8,
                      padding: "12px 16px", borderBottom: `1px solid ${t.separatorLight}`,
                    }}>
                      <div style={{
                        width: 28, height: 28, borderRadius: "50%",
                        background: `radial-gradient(circle, ${f.primary}, ${f.primary}66)`,
                        boxShadow: `0 0 10px ${f.primary}33`,
                      }} />
                      <span style={{ fontSize: 14, color: t.green, fontWeight: 500, flex: 1 }}>Ready</span>
                      <div style={{ display: "flex", gap: 6 }}>
                        <div style={{
                          padding: "4px 10px", borderRadius: 10,
                          background: `${t.elevated}88`, border: `1px solid ${t.separator}`,
                          fontSize: 12, color: t.secondary,
                        }}>Full</div>
                        <div style={{
                          padding: "4px 10px", borderRadius: 10,
                          background: `${t.elevated}88`, border: `1px solid ${t.separator}`,
                          fontSize: 12, color: t.secondary,
                        }}>Bubble</div>
                      </div>
                    </div>
                    {/* Transcript */}
                    <div style={{ flex: 1, padding: 12, overflow: "auto" }}>
                      <div style={{ fontSize: 13, color: t.secondary, marginBottom: 6 }}>
                        <span style={{ color: t.labelTertiary }}>{">"}</span> Set reminder for 3pm
                      </div>
                      <div style={{ fontSize: 13, color: t.secondary, marginBottom: 6 }}>
                        <span style={{ color: t.green }}>-</span> Created reminder ✓
                      </div>
                    </div>
                    {/* Input */}
                    <div style={{
                      padding: "8px 12px 20px", display: "flex", gap: 8,
                      borderTop: `1px solid ${t.separatorLight}`,
                    }}>
                      <div style={{
                        flex: 1, padding: "8px 12px", borderRadius: 16,
                        background: `${t.elevated}88`, border: `1px solid ${t.separator}`,
                        fontSize: 13, color: t.labelTertiary,
                      }}>Ask...</div>
                      <div style={{
                        width: 30, height: 30, borderRadius: 15,
                        background: f.primary, display: "flex", alignItems: "center", justifyContent: "center",
                        fontSize: 14, color: f.dark,
                      }}>↑</div>
                    </div>
                  </div>
                )}
              </PhoneDesktop>
            </div>
          </>
        ) : (
          /* Proposed overlays */
          <>
            <div>
              <div style={{ fontSize: 12, color: t.labelTertiary, marginBottom: 8, fontWeight: 600, textAlign: "center" }}>
                PROPOSED · {overlayMode === "bubble" ? "Minimal Bubble" : overlayMode === "mini" ? "Slide-up Panel" : "Dynamic Island"}
              </div>
              <PhoneDesktop flavor={flavor}>
                {overlayMode === "bubble" ? (
                  /* Proposed bubble: clean, no particle orb */
                  <div style={{ position: "absolute", bottom: 100, right: 20 }}>
                    <div style={{
                      width: 52, height: 52, borderRadius: "50%",
                      background: f.primary,
                      display: "flex", alignItems: "center", justifyContent: "center",
                      boxShadow: `0 4px 16px rgba(0,0,0,0.3)`,
                    }}>
                      <div style={{ width: 22, height: 22, borderRadius: "50%", background: "rgba(0,0,0,0.15)" }} />
                    </div>
                    {/* Minimal badge */}
                    <div style={{
                      position: "absolute", top: -2, right: -2,
                      width: 16, height: 16, borderRadius: "50%",
                      background: t.green, display: "flex", alignItems: "center", justifyContent: "center",
                      border: "2px solid #1a1a2e",
                    }}>
                      <span style={{ fontSize: 9, color: "#fff", fontWeight: 700 }}>✓</span>
                    </div>
                  </div>
                ) : overlayMode === "mini" ? (
                  /* Proposed mini: iOS-style slide-up */
                  <div style={{
                    position: "absolute", bottom: 0, left: 0, right: 0,
                    borderRadius: "16px 16px 0 0",
                    background: `rgba(28,28,30,0.92)`,
                    backdropFilter: "blur(40px)",
                    display: "flex", flexDirection: "column",
                    overflow: "hidden",
                  }}>
                    {/* Grab handle */}
                    <div style={{ padding: "8px 0 4px", display: "flex", justifyContent: "center" }}>
                      <div style={{ width: 36, height: 5, borderRadius: 3, background: t.tertiary }} />
                    </div>
                    {/* Compact header */}
                    <div style={{
                      display: "flex", alignItems: "center", gap: 10,
                      padding: "6px 16px 10px",
                    }}>
                      <div style={{
                        width: 28, height: 28, borderRadius: "50%", background: f.primary,
                        display: "flex", alignItems: "center", justifyContent: "center",
                      }}>
                        <div style={{ width: 12, height: 12, borderRadius: "50%", background: "rgba(0,0,0,0.15)" }} />
                      </div>
                      <span style={{ fontSize: 15, fontWeight: 600, flex: 1, letterSpacing: -0.2 }}>Fawx</span>
                      <button style={{
                        background: t.elevated, border: "none", borderRadius: 14,
                        padding: "5px 12px", fontSize: 13, color: t.secondary, cursor: "pointer",
                      }}>Expand</button>
                    </div>
                    {/* Messages - clean list */}
                    <div style={{ padding: "0 16px 8px" }}>
                      <div style={{ display: "flex", gap: 8, marginBottom: 10 }}>
                        <div style={{
                          padding: "8px 12px", borderRadius: "14px 14px 14px 4px",
                          background: t.elevated, fontSize: 14, color: t.label,
                          maxWidth: "80%",
                        }}>
                          Reminder set for 3:00 PM
                        </div>
                      </div>
                      <div style={{
                        display: "inline-flex", alignItems: "center", gap: 6,
                        padding: "5px 10px", borderRadius: 12,
                        background: `${t.green}18`,
                      }}>
                        <span style={{ fontSize: 11, color: t.green, fontWeight: 500 }}>✓ Completed</span>
                      </div>
                    </div>
                    {/* Input */}
                    <div style={{
                      padding: "8px 12px 24px", display: "flex", gap: 8,
                      borderTop: `0.5px solid ${t.separatorLight}`,
                    }}>
                      <div style={{
                        flex: 1, padding: "9px 14px", borderRadius: 20,
                        background: t.elevated, fontSize: 14, color: t.labelTertiary,
                      }}>Message</div>
                      <div style={{
                        width: 34, height: 34, borderRadius: "50%", background: f.primary,
                        display: "flex", alignItems: "center", justifyContent: "center",
                      }}>
                        <span style={{ fontSize: 16, color: f.dark }}>↑</span>
                      </div>
                    </div>
                  </div>
                ) : (
                  /* Proposed: Dynamic Island style */
                  <>
                    <div style={{
                      position: "absolute", top: 12, left: "50%", transform: "translateX(-50%)",
                      display: "flex", alignItems: "center", gap: 10,
                      padding: "8px 14px 8px 10px",
                      borderRadius: 28,
                      background: "rgba(0,0,0,0.85)",
                      backdropFilter: "blur(40px)",
                      boxShadow: "0 4px 20px rgba(0,0,0,0.4)",
                      minWidth: 200,
                    }}>
                      <div style={{
                        width: 32, height: 32, borderRadius: "50%", background: f.primary,
                        display: "flex", alignItems: "center", justifyContent: "center",
                        flexShrink: 0,
                      }}>
                        <div style={{ width: 14, height: 14, borderRadius: "50%", background: "rgba(0,0,0,0.15)" }} />
                      </div>
                      <div style={{ flex: 1 }}>
                        <div style={{ fontSize: 13, fontWeight: 600, color: "#fff", letterSpacing: -0.2 }}>
                          Reminder set
                        </div>
                        <div style={{ fontSize: 11, color: "rgba(255,255,255,0.5)" }}>
                          Meeting with Sarah · 3:00 PM
                        </div>
                      </div>
                      <div style={{
                        width: 24, height: 24, borderRadius: "50%", background: t.green,
                        display: "flex", alignItems: "center", justifyContent: "center",
                      }}>
                        <span style={{ fontSize: 12, color: "#fff", fontWeight: 700 }}>✓</span>
                      </div>
                    </div>
                  </>
                )}
              </PhoneDesktop>
            </div>

            {/* Side-by-side states for proposed */}
            <div>
              <div style={{ fontSize: 12, color: t.labelTertiary, marginBottom: 8, fontWeight: 600, textAlign: "center" }}>
                PROPOSED · State Variations
              </div>
              <div style={{
                display: "flex", flexDirection: "column", gap: 16,
                background: t.surface, borderRadius: 20, padding: 20, width: 340,
              }}>
                {/* Idle */}
                <div>
                  <div style={{ fontSize: 11, color: t.labelTertiary, marginBottom: 6, fontWeight: 600 }}>IDLE</div>
                  <div style={{
                    display: "flex", alignItems: "center", gap: 10,
                    padding: "10px 14px", borderRadius: 22,
                    background: "rgba(0,0,0,0.7)", backdropFilter: "blur(20px)",
                  }}>
                    <div style={{ width: 28, height: 28, borderRadius: "50%", background: f.primary,
                      display: "flex", alignItems: "center", justifyContent: "center" }}>
                      <div style={{ width: 12, height: 12, borderRadius: "50%", background: "rgba(0,0,0,0.15)" }} />
                    </div>
                    <span style={{ fontSize: 14, fontWeight: 500, color: "#fff" }}>Fawx</span>
                  </div>
                </div>

                {/* Executing */}
                <div>
                  <div style={{ fontSize: 11, color: t.labelTertiary, marginBottom: 6, fontWeight: 600 }}>EXECUTING</div>
                  <div style={{
                    display: "flex", alignItems: "center", gap: 10,
                    padding: "10px 14px", borderRadius: 22,
                    background: "rgba(0,0,0,0.7)", backdropFilter: "blur(20px)",
                  }}>
                    <div style={{ width: 28, height: 28, borderRadius: "50%", background: f.primary,
                      display: "flex", alignItems: "center", justifyContent: "center", position: "relative" }}>
                      <div style={{ width: 12, height: 12, borderRadius: "50%", background: "rgba(0,0,0,0.15)" }} />
                      {/* Spinning ring */}
                      <div style={{
                        position: "absolute", inset: -3,
                        borderRadius: "50%", border: `2px solid transparent`,
                        borderTopColor: f.primary,
                        animation: "spin 0.8s linear infinite",
                      }} />
                    </div>
                    <div style={{ flex: 1 }}>
                      <span style={{ fontSize: 13, fontWeight: 600, color: "#fff" }}>Opening Calendar...</span>
                    </div>
                    <button style={{
                      background: t.red, border: "none", borderRadius: 12,
                      padding: "4px 10px", fontSize: 12, color: "#fff", cursor: "pointer", fontWeight: 600,
                    }}>Stop</button>
                  </div>
                </div>

                {/* Completed */}
                <div>
                  <div style={{ fontSize: 11, color: t.labelTertiary, marginBottom: 6, fontWeight: 600 }}>COMPLETED</div>
                  <div style={{
                    display: "flex", alignItems: "center", gap: 10,
                    padding: "10px 14px", borderRadius: 22,
                    background: "rgba(0,0,0,0.7)", backdropFilter: "blur(20px)",
                  }}>
                    <div style={{ width: 28, height: 28, borderRadius: "50%", background: f.primary,
                      display: "flex", alignItems: "center", justifyContent: "center" }}>
                      <div style={{ width: 12, height: 12, borderRadius: "50%", background: "rgba(0,0,0,0.15)" }} />
                    </div>
                    <div style={{ flex: 1 }}>
                      <div style={{ fontSize: 13, fontWeight: 600, color: "#fff" }}>Reminder set</div>
                      <div style={{ fontSize: 11, color: "rgba(255,255,255,0.4)" }}>3:00 PM · Meeting with Sarah</div>
                    </div>
                    <div style={{ width: 22, height: 22, borderRadius: "50%", background: t.green,
                      display: "flex", alignItems: "center", justifyContent: "center" }}>
                      <span style={{ fontSize: 11, color: "#fff", fontWeight: 700 }}>✓</span>
                    </div>
                  </div>
                </div>

                {/* Failed */}
                <div>
                  <div style={{ fontSize: 11, color: t.labelTertiary, marginBottom: 6, fontWeight: 600 }}>FAILED</div>
                  <div style={{
                    display: "flex", alignItems: "center", gap: 10,
                    padding: "10px 14px", borderRadius: 22,
                    background: "rgba(0,0,0,0.7)", backdropFilter: "blur(20px)",
                  }}>
                    <div style={{ width: 28, height: 28, borderRadius: "50%", background: t.red,
                      display: "flex", alignItems: "center", justifyContent: "center" }}>
                      <span style={{ fontSize: 14, color: "#fff", fontWeight: 700 }}>!</span>
                    </div>
                    <div style={{ flex: 1 }}>
                      <div style={{ fontSize: 13, fontWeight: 600, color: "#fff" }}>Calendar access denied</div>
                      <div style={{ fontSize: 11, color: "rgba(255,255,255,0.4)" }}>Tap to open settings</div>
                    </div>
                  </div>
                </div>

                <style>{`
                  @keyframes spin { from { transform: rotate(0deg); } to { transform: rotate(360deg); } }
                `}</style>
              </div>
            </div>
          </>
        )}
      </div>

      {/* Notes */}
      <div style={{
        maxWidth: 800, width: "100%", marginTop: 24, padding: 20,
        background: t.surface, borderRadius: 16, fontSize: 13,
        color: t.secondary, lineHeight: 1.6,
      }}>
        <div style={{ fontWeight: 600, color: t.label, marginBottom: 8, fontSize: 15 }}>
          Overlay Design Recommendations
        </div>
        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 16 }}>
          <div>
            <div style={{ fontWeight: 600, color: f.primary, marginBottom: 4, fontSize: 13 }}>Dynamic Island (New)</div>
            <p style={{ margin: 0 }}>
              Inspired by the iPhone's Dynamic Island. Fawx lives at the top of the screen as a compact pill that expands contextually — showing status during execution, results on completion, and collapsing to minimal when idle. Feels native to iOS users.
            </p>
          </div>
          <div>
            <div style={{ fontWeight: 600, color: f.primary, marginBottom: 4, fontSize: 13 }}>Slide-up Panel (Revised Mini)</div>
            <p style={{ margin: 0 }}>
              Replaces the current mini-chat with an iOS-style sheet: grab handle, frosted background, tighter typography. The "Full" and "Bubble" toggle buttons become a single "Expand" control. Last message + status shown inline instead of transcript-style.
            </p>
          </div>
          <div>
            <div style={{ fontWeight: 600, color: f.primary, marginBottom: 4, fontSize: 13 }}>Minimal Bubble (Revised)</div>
            <p style={{ margin: 0 }}>
              Drops the particle-filled orb and progress ring SVG for a flat, solid-color circle. Badge is smaller and cleaner. The long-press context menu stays, but the visual weight is halved. Feels like a system affordance, not a decoration.
            </p>
          </div>
          <div>
            <div style={{ fontWeight: 600, color: f.primary, marginBottom: 4, fontSize: 13 }}>State Communication</div>
            <p style={{ margin: 0 }}>
              All three modes share the same state vocabulary: color (primary=idle, animated=executing, green=done, red=failed) and a consistent text label. Users learn one mental model regardless of which overlay they prefer.
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}