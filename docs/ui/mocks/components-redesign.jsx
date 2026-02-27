import { useState } from "react";

const FLAVORS = {
  lemon: { primary: "#FFD600", glow: "#FFF9C4", tint: "#332B00", dark: "#1a1a00" },
  tangerine: { primary: "#FF8C00", glow: "#FFE0B2", tint: "#331C00", dark: "#fff" },
  lime: { primary: "#7CB342", glow: "#DCEDC8", tint: "#1A2E0D", dark: "#fff" },
  blood_orange: { primary: "#D84315", glow: "#FFCCBC", tint: "#2E0D04", dark: "#fff" },
  grapefruit: { primary: "#E91E63", glow: "#F8BBD0", tint: "#2E0413", dark: "#fff" },
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
  blue: "#0A84FF",
};

// --- Component Showcase ---
const Section = ({ title, description, children }) => (
  <div style={{ marginBottom: 40 }}>
    <h3 style={{ fontSize: 18, fontWeight: 700, letterSpacing: -0.3, margin: "0 0 4px", color: t.label }}>{title}</h3>
    {description && <p style={{ fontSize: 13, color: t.secondary, margin: "0 0 16px", lineHeight: 1.5 }}>{description}</p>}
    {children}
  </div>
);

const PhoneFrame = ({ children, height = 360 }) => (
  <div style={{
    width: 390, borderRadius: 24, overflow: "hidden",
    border: "2px solid #333", background: t.bg,
    boxShadow: "0 10px 40px rgba(0,0,0,0.4)", height,
  }}>{children}</div>
);

export default function ComponentsRedesign() {
  const [flavor, setFlavor] = useState("tangerine");
  const f = FLAVORS[flavor];

  return (
    <div style={{
      fontFamily: "-apple-system, SF Pro Display, SF Pro Text, system-ui, sans-serif",
      background: "#111", minHeight: "100vh", padding: "32px 24px", color: t.label,
      maxWidth: 800, margin: "0 auto",
    }}>
      <h1 style={{ fontSize: 28, fontWeight: 800, letterSpacing: -0.8, margin: "0 0 4px" }}>
        Component Library Alternatives
      </h1>
      <p style={{ fontSize: 15, color: t.secondary, margin: "0 0 20px" }}>
        Side-by-side current vs proposed for individual UI components
      </p>

      {/* Flavor picker */}
      <div style={{ display: "flex", gap: 8, marginBottom: 32, alignItems: "center" }}>
        <span style={{ fontSize: 13, color: t.labelTertiary, marginRight: 4 }}>Flavor:</span>
        {Object.entries(FLAVORS).map(([name, fl]) => (
          <button key={name} onClick={() => setFlavor(name)} style={{
            width: 26, height: 26, borderRadius: 13, cursor: "pointer",
            background: fl.primary, border: flavor === name ? `2px solid #fff` : "2px solid transparent",
            transition: "all 0.15s", transform: flavor === name ? "scale(1.15)" : "scale(1)",
          }} />
        ))}
      </div>

      {/* 1. Message Bubbles */}
      <Section
        title="1. Message Bubbles — Three Variants"
        description="Current uses glass morphism with glow borders. Proposed A is iMessage-style solid colors. Proposed B is a WhatsApp-style minimal approach with tinted backgrounds."
      >
        <div style={{ display: "flex", gap: 16, flexWrap: "wrap" }}>
          {/* Current */}
          <div>
            <div style={{ fontSize: 12, color: t.labelTertiary, marginBottom: 8, fontWeight: 600 }}>CURRENT · Glass</div>
            <PhoneFrame height={280}>
              <div style={{ padding: 16, display: "flex", flexDirection: "column", gap: 8 }}>
                {/* User */}
                <div style={{ alignSelf: "flex-end", maxWidth: "78%" }}>
                  <div style={{
                    padding: "11px 15px", borderRadius: "16px 16px 4px 16px",
                    background: `linear-gradient(135deg, ${f.tint}cc, ${f.tint}88)`,
                    border: `1px solid ${f.primary}6b`,
                    boxShadow: `0 0 16px ${f.primary}15`,
                    position: "relative", overflow: "hidden",
                  }}>
                    <div style={{ position: "absolute", inset: 0, background: "linear-gradient(180deg, rgba(255,255,255,0.05) 0%, transparent 40%)", pointerEvents: "none" }} />
                    <span style={{ fontSize: 15, color: t.label, position: "relative" }}>Set a reminder for 3pm</span>
                  </div>
                </div>
                {/* Assistant */}
                <div style={{ alignSelf: "flex-start", maxWidth: "78%" }}>
                  <div style={{
                    padding: "11px 15px", borderRadius: "16px 16px 16px 4px",
                    background: `${t.surface}dd`, border: `1px solid ${t.separator}`,
                    backdropFilter: "blur(20px)",
                  }}>
                    <span style={{ fontSize: 15, color: t.secondary }}>Done! Reminder set for 3:00 PM today.</span>
                  </div>
                </div>
                {/* Action */}
                <div style={{ alignSelf: "flex-start", maxWidth: "78%" }}>
                  <div style={{
                    padding: "9px 13px", borderRadius: "14px 14px 14px 4px",
                    background: `${t.elevated}80`, border: `1px solid ${f.primary}57`,
                  }}>
                    <span style={{ fontSize: 13, color: t.labelTertiary }}>↗ calendar · Created reminder</span>
                  </div>
                </div>
              </div>
            </PhoneFrame>
          </div>

          {/* Proposed A: iMessage */}
          <div>
            <div style={{ fontSize: 12, color: t.labelTertiary, marginBottom: 8, fontWeight: 600 }}>PROPOSED A · Solid</div>
            <PhoneFrame height={280}>
              <div style={{ padding: 16, display: "flex", flexDirection: "column", gap: 6 }}>
                <div style={{ alignSelf: "flex-end", maxWidth: "78%" }}>
                  <div style={{
                    padding: "10px 14px", borderRadius: "18px 18px 4px 18px",
                    background: f.primary,
                  }}>
                    <span style={{ fontSize: 15, color: f.dark }}>Set a reminder for 3pm</span>
                  </div>
                </div>
                <div style={{ alignSelf: "flex-start", maxWidth: "78%" }}>
                  <div style={{
                    padding: "10px 14px", borderRadius: "18px 18px 18px 4px",
                    background: t.elevated,
                  }}>
                    <span style={{ fontSize: 15, color: t.label }}>Done! Reminder set for 3:00 PM today.</span>
                  </div>
                </div>
                {/* Action inline */}
                <div style={{ alignSelf: "flex-start" }}>
                  <div style={{
                    display: "inline-flex", alignItems: "center", gap: 6,
                    padding: "6px 12px", borderRadius: 14,
                    background: `${f.primary}15`,
                  }}>
                    <span style={{ fontSize: 12, color: f.primary }}>📅</span>
                    <span style={{ fontSize: 12, color: t.secondary }}>Reminder created</span>
                  </div>
                </div>
              </div>
            </PhoneFrame>
          </div>

          {/* Proposed B: Minimal tinted */}
          <div>
            <div style={{ fontSize: 12, color: t.labelTertiary, marginBottom: 8, fontWeight: 600 }}>PROPOSED B · Tinted Minimal</div>
            <PhoneFrame height={280}>
              <div style={{ padding: 16, display: "flex", flexDirection: "column", gap: 6 }}>
                <div style={{ alignSelf: "flex-end", maxWidth: "78%" }}>
                  <div style={{
                    padding: "10px 14px", borderRadius: "20px 20px 6px 20px",
                    background: `${f.primary}22`, border: `1px solid ${f.primary}33`,
                  }}>
                    <span style={{ fontSize: 15, color: t.label }}>Set a reminder for 3pm</span>
                  </div>
                </div>
                <div style={{ alignSelf: "flex-start", maxWidth: "78%" }}>
                  <div style={{
                    padding: "10px 14px", borderRadius: "20px 20px 20px 6px",
                    background: t.elevated,
                  }}>
                    <span style={{ fontSize: 15, color: t.label }}>Done! Reminder set for 3:00 PM today.</span>
                  </div>
                </div>
                <div style={{ alignSelf: "flex-start", paddingLeft: 4 }}>
                  <span style={{ fontSize: 12, color: t.labelTertiary }}>📅 Reminder created · just now</span>
                </div>
              </div>
            </PhoneFrame>
          </div>
        </div>
      </Section>

      {/* 2. Input Bar */}
      <Section
        title="2. Input Bar — Four Variants"
        description="The input bar is where users spend the most time. Current has glass styling with separate mic/send buttons."
      >
        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 16 }}>
          {/* Current */}
          <div>
            <div style={{ fontSize: 12, color: t.labelTertiary, marginBottom: 8, fontWeight: 600 }}>CURRENT</div>
            <div style={{
              padding: "10px 12px", borderRadius: 20, background: t.bg,
              border: `1px solid ${t.separator}`,
            }}>
              <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                <div style={{
                  flex: 1, padding: "10px 14px", borderRadius: 20,
                  background: `linear-gradient(135deg, ${t.elevated}cc, ${t.surface}88)`,
                  border: `1px solid ${t.separator}`, backdropFilter: "blur(10px)",
                  fontSize: 14, color: t.labelTertiary,
                }}>Ask anything...</div>
                <div style={{
                  width: 34, height: 34, borderRadius: 17,
                  background: `linear-gradient(135deg, ${f.primary}, ${f.tint})`,
                  border: `1px solid ${f.primary}66`,
                  display: "flex", alignItems: "center", justifyContent: "center",
                  fontSize: 15, boxShadow: `0 0 10px ${f.primary}33`,
                }}>🎤</div>
                <div style={{
                  width: 34, height: 34, borderRadius: 17,
                  background: `${t.elevated}88`, border: `1px solid ${t.separator}`,
                  display: "flex", alignItems: "center", justifyContent: "center",
                  fontSize: 15,
                }}>↑</div>
              </div>
            </div>
          </div>

          {/* Proposed A: iMessage */}
          <div>
            <div style={{ fontSize: 12, color: t.labelTertiary, marginBottom: 8, fontWeight: 600 }}>PROPOSED A · iMessage</div>
            <div style={{
              padding: "8px 12px", borderRadius: 20, background: t.bg,
              borderTop: `0.5px solid ${t.separatorLight}`,
            }}>
              <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                <div style={{
                  flex: 1, padding: "10px 16px", borderRadius: 22,
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
          </div>

          {/* Proposed B: Floating */}
          <div>
            <div style={{ fontSize: 12, color: t.labelTertiary, marginBottom: 8, fontWeight: 600 }}>PROPOSED B · Floating Pill</div>
            <div style={{ padding: "10px 16px", background: t.bg }}>
              <div style={{
                display: "flex", gap: 8, alignItems: "center",
                padding: "6px 6px 6px 18px", borderRadius: 28,
                background: t.surface, border: `1px solid ${t.separatorLight}`,
              }}>
                <span style={{ flex: 1, fontSize: 14, color: t.labelTertiary }}>Ask Fawx...</span>
                <div style={{
                  width: 38, height: 38, borderRadius: "50%", background: f.primary,
                  display: "flex", alignItems: "center", justifyContent: "center",
                }}>
                  <span style={{ fontSize: 17, color: f.dark }}>↑</span>
                </div>
              </div>
            </div>
          </div>

          {/* Proposed C: Expandable */}
          <div>
            <div style={{ fontSize: 12, color: t.labelTertiary, marginBottom: 8, fontWeight: 600 }}>PROPOSED C · Toolbar</div>
            <div style={{ padding: "8px 12px", background: t.bg, borderTop: `0.5px solid ${t.separatorLight}` }}>
              <div style={{ display: "flex", gap: 6, marginBottom: 8 }}>
                {["📎", "📷", "🎤"].map((icon, i) => (
                  <div key={i} style={{
                    width: 32, height: 32, borderRadius: "50%", background: t.elevated,
                    display: "flex", alignItems: "center", justifyContent: "center",
                    fontSize: 14, cursor: "pointer",
                  }}>{icon}</div>
                ))}
              </div>
              <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                <div style={{
                  flex: 1, padding: "10px 16px", borderRadius: 22,
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
          </div>
        </div>
      </Section>

      {/* 3. Action/Tool Indicators */}
      <Section
        title="3. Tool Action Indicators"
        description="When Fawx executes a phone action (calendar, settings, messages), how should it appear in the chat?"
      >
        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 16 }}>
          {/* Current: bubble */}
          <div>
            <div style={{ fontSize: 12, color: t.labelTertiary, marginBottom: 8, fontWeight: 600 }}>CURRENT · Action Bubble</div>
            <div style={{ background: t.bg, borderRadius: 16, padding: 16 }}>
              <div style={{
                padding: "10px 14px", borderRadius: "14px 14px 14px 4px",
                background: `${t.elevated}80`, border: `1px solid ${f.primary}57`,
                maxWidth: "80%",
              }}>
                <span style={{ fontSize: 14, color: t.secondary }}>Created reminder: "Meeting with Sarah" at 3:00 PM</span>
              </div>
              <span style={{ fontSize: 11, color: t.labelTertiary, marginTop: 3, display: "block", paddingLeft: 4 }}>↗ calendar</span>
            </div>
          </div>

          {/* Proposed: inline chip */}
          <div>
            <div style={{ fontSize: 12, color: t.labelTertiary, marginBottom: 8, fontWeight: 600 }}>PROPOSED · Inline Chip</div>
            <div style={{ background: t.bg, borderRadius: 16, padding: 16 }}>
              <div style={{
                display: "inline-flex", alignItems: "center", gap: 8,
                padding: "8px 14px", borderRadius: 20,
                background: `${f.primary}12`, border: `1px solid ${f.primary}22`,
              }}>
                <span style={{ fontSize: 14 }}>📅</span>
                <span style={{ fontSize: 13, color: t.label, fontWeight: 500 }}>Set reminder</span>
                <span style={{ fontSize: 12, color: t.green }}>✓</span>
              </div>
              <div style={{ marginTop: 8, paddingLeft: 4 }}>
                <span style={{ fontSize: 12, color: t.labelTertiary }}>"Meeting with Sarah" · 3:00 PM</span>
              </div>
            </div>
          </div>

          {/* Proposed: timeline */}
          <div style={{ gridColumn: "span 2" }}>
            <div style={{ fontSize: 12, color: t.labelTertiary, marginBottom: 8, fontWeight: 600 }}>PROPOSED · Execution Timeline</div>
            <div style={{ background: t.bg, borderRadius: 16, padding: 16 }}>
              <div style={{ display: "flex", flexDirection: "column", gap: 0, paddingLeft: 20, position: "relative" }}>
                {/* Vertical line */}
                <div style={{
                  position: "absolute", left: 7, top: 8, bottom: 8, width: 1.5,
                  background: `${f.primary}33`,
                }} />
                {[
                  { label: "Open Calendar", status: "done", detail: "200ms" },
                  { label: "Create event", status: "done", detail: "450ms" },
                  { label: "Set reminder", status: "done", detail: "120ms" },
                ].map((step, i) => (
                  <div key={i} style={{ display: "flex", alignItems: "center", gap: 12, padding: "6px 0", position: "relative" }}>
                    <div style={{
                      position: "absolute", left: -16, width: 10, height: 10, borderRadius: "50%",
                      background: step.status === "done" ? t.green : t.tertiary,
                      border: `2px solid ${t.bg}`,
                    }} />
                    <span style={{ fontSize: 14, color: t.label, fontWeight: 500 }}>{step.label}</span>
                    <span style={{ fontSize: 12, color: t.labelTertiary }}>{step.detail}</span>
                    {step.status === "done" && <span style={{ fontSize: 12, color: t.green }}>✓</span>}
                  </div>
                ))}
              </div>
            </div>
          </div>
        </div>
      </Section>

      {/* 4. Model Switcher */}
      <Section
        title="4. Model Switcher"
        description="Current: small chip in the top bar. Alternative: a bottom sheet picker or segmented control."
      >
        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr", gap: 16 }}>
          <div>
            <div style={{ fontSize: 12, color: t.labelTertiary, marginBottom: 8, fontWeight: 600 }}>CURRENT · Top Chip</div>
            <div style={{ background: t.bg, borderRadius: 16, padding: 16 }}>
              <div style={{
                display: "inline-flex", padding: "6px 14px", borderRadius: 14,
                background: `linear-gradient(135deg, ${t.elevated}cc, ${t.surface}88)`,
                border: `1px solid ${f.primary}33`,
                fontSize: 13, color: t.secondary,
              }}>Claude Sonnet ▾</div>
            </div>
          </div>
          <div>
            <div style={{ fontSize: 12, color: t.labelTertiary, marginBottom: 8, fontWeight: 600 }}>ALT · Segmented</div>
            <div style={{ background: t.bg, borderRadius: 16, padding: 16 }}>
              <div style={{
                display: "inline-flex", background: t.elevated, borderRadius: 10, padding: 3,
              }}>
                {["Local", "Sonnet", "Opus"].map((m, i) => (
                  <div key={m} style={{
                    padding: "7px 14px", borderRadius: 8, fontSize: 13, fontWeight: 500,
                    background: i === 1 ? t.tertiary : "transparent",
                    color: i === 1 ? t.label : t.secondary,
                    cursor: "pointer",
                  }}>{m}</div>
                ))}
              </div>
            </div>
          </div>
          <div>
            <div style={{ fontSize: 12, color: t.labelTertiary, marginBottom: 8, fontWeight: 600 }}>ALT · Contextual Menu</div>
            <div style={{ background: t.bg, borderRadius: 16, padding: 12 }}>
              <div style={{
                background: t.surface, borderRadius: 14, overflow: "hidden",
                boxShadow: "0 8px 30px rgba(0,0,0,0.5)",
              }}>
                {[
                  { name: "llama.cpp", desc: "On-device · Fast", icon: "📱" },
                  { name: "Claude Sonnet", desc: "Cloud · Balanced", icon: "⚡", selected: true },
                  { name: "Claude Opus", desc: "Cloud · Powerful", icon: "🧠" },
                ].map((model, i) => (
                  <div key={i} style={{
                    display: "flex", alignItems: "center", gap: 10,
                    padding: "11px 14px",
                    borderBottom: i < 2 ? `0.5px solid ${t.separatorLight}` : "none",
                    background: model.selected ? `${f.primary}12` : "transparent",
                  }}>
                    <span style={{ fontSize: 16 }}>{model.icon}</span>
                    <div style={{ flex: 1 }}>
                      <div style={{ fontSize: 14, color: t.label, fontWeight: model.selected ? 600 : 400 }}>{model.name}</div>
                      <div style={{ fontSize: 11, color: t.labelTertiary }}>{model.desc}</div>
                    </div>
                    {model.selected && <span style={{ color: f.primary, fontWeight: 600 }}>✓</span>}
                  </div>
                ))}
              </div>
            </div>
          </div>
        </div>
      </Section>

      {/* 5. Loading/Thinking States */}
      <Section
        title="5. Thinking / Loading States"
        description="Current uses pulsing dots. Alternatives: typing indicator bar, skeleton bubbles, or status text."
      >
        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr", gap: 16 }}>
          <div>
            <div style={{ fontSize: 12, color: t.labelTertiary, marginBottom: 8, fontWeight: 600 }}>CURRENT · Pulsing Dots</div>
            <div style={{ background: t.bg, borderRadius: 16, padding: 20, display: "flex", gap: 6 }}>
              {[0, 1, 2].map(i => (
                <div key={i} style={{
                  width: 8, height: 8, borderRadius: "50%",
                  background: f.primary,
                  opacity: 0.4 + (i * 0.2),
                  animation: `pulse 1.2s ease-in-out ${i * 0.15}s infinite`,
                }} />
              ))}
            </div>
          </div>
          <div>
            <div style={{ fontSize: 12, color: t.labelTertiary, marginBottom: 8, fontWeight: 600 }}>ALT · Typing Bar</div>
            <div style={{ background: t.bg, borderRadius: 16, padding: 20 }}>
              <div style={{
                display: "inline-flex", alignItems: "center", gap: 8,
                padding: "8px 14px", borderRadius: 16, background: t.elevated,
              }}>
                <div style={{ width: 6, height: 6, borderRadius: "50%", background: f.primary }} />
                <span style={{ fontSize: 13, color: t.secondary }}>Fawx is thinking...</span>
              </div>
            </div>
          </div>
          <div>
            <div style={{ fontSize: 12, color: t.labelTertiary, marginBottom: 8, fontWeight: 600 }}>ALT · Status Line</div>
            <div style={{ background: t.bg, borderRadius: 16, padding: 20 }}>
              <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                <div style={{
                  width: 16, height: 2, borderRadius: 1, background: f.primary,
                  animation: "expand 1s ease-in-out infinite alternate",
                }} />
                <span style={{ fontSize: 12, color: t.labelTertiary, fontStyle: "italic" }}>
                  Checking your calendar...
                </span>
              </div>
            </div>
          </div>
        </div>
      </Section>
    </div>
  );
}