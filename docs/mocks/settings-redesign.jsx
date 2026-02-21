import { useState } from "react";

const FLAVORS = {
  lemon: { primary: "#FFD600", label: "Lemon" },
  tangerine: { primary: "#FF8C00", label: "Tangerine" },
  lime: { primary: "#7CB342", label: "Lime" },
  blood_orange: { primary: "#D84315", label: "Blood Orange" },
  grapefruit: { primary: "#E91E63", label: "Grapefruit" },
};

const t = {
  bg: "#000000",
  surface: "#1C1C1E",
  surfaceElevated: "#2C2C2E",
  surfaceTertiary: "#3A3A3C",
  label: "#FFFFFF",
  labelSecondary: "rgba(235,235,245,0.6)",
  labelTertiary: "rgba(235,235,245,0.3)",
  separator: "rgba(84,84,88,0.65)",
  separatorLight: "rgba(84,84,88,0.35)",
  systemBlue: "#0A84FF",
  systemGreen: "#30D158",
  systemRed: "#FF453A",
  systemOrange: "#FF9F0A",
};

// Reusable iOS grouped list section
const GroupedSection = ({ title, children }) => (
  <div style={{ marginBottom: 28 }}>
    {title && (
      <div style={{
        fontSize: 13, fontWeight: 400, color: t.labelSecondary,
        textTransform: "uppercase", letterSpacing: 0.5,
        paddingLeft: 16, marginBottom: 8,
      }}>{title}</div>
    )}
    <div style={{
      background: t.surface, borderRadius: 12, overflow: "hidden",
    }}>{children}</div>
  </div>
);

const GroupedRow = ({ label, detail, trailing, showChevron = false, showSep = true, onClick, destructive }) => (
  <div
    onClick={onClick}
    style={{
      display: "flex", alignItems: "center", padding: "13px 16px",
      borderBottom: showSep ? `0.5px solid ${t.separatorLight}` : "none",
      cursor: onClick ? "pointer" : "default",
      transition: "background 0.1s",
    }}
  >
    <div style={{ flex: 1 }}>
      <div style={{ fontSize: 16, color: destructive ? t.systemRed : t.label, letterSpacing: -0.2 }}>{label}</div>
      {detail && <div style={{ fontSize: 13, color: t.labelTertiary, marginTop: 2 }}>{detail}</div>}
    </div>
    {trailing}
    {showChevron && (
      <svg width="8" height="13" viewBox="0 0 8 13" fill="none" style={{ marginLeft: 8, opacity: 0.3 }}>
        <path d="M1 1l5.5 5.5L1 12" stroke={t.label} strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
      </svg>
    )}
  </div>
);

const Toggle = ({ on, accent }) => (
  <div style={{
    width: 51, height: 31, borderRadius: 16,
    background: on ? (accent || t.systemGreen) : t.surfaceTertiary,
    padding: 2, cursor: "pointer", transition: "background 0.2s",
    display: "flex", alignItems: "center",
  }}>
    <div style={{
      width: 27, height: 27, borderRadius: 14, background: "#fff",
      transform: on ? "translateX(20px)" : "translateX(0)",
      transition: "transform 0.2s ease", boxShadow: "0 1px 3px rgba(0,0,0,0.3)",
    }} />
  </div>
);

const Badge = ({ text, color }) => (
  <span style={{
    fontSize: 12, fontWeight: 500, padding: "3px 8px",
    borderRadius: 6, background: `${color}22`, color,
  }}>{text}</span>
);

export default function SettingsRedesign() {
  const [view, setView] = useState("proposed"); // "current" or "proposed"
  const [flavor, setFlavor] = useState("tangerine");
  const [subPage, setSubPage] = useState(null); // null = hub, "appearance", "trust", "phone", "sound", "models"
  const f = FLAVORS[flavor];

  const phone = { width: 390, height: 844 };

  const CurrentSettingsHub = () => (
    <div style={{ padding: 16, overflowY: "auto", flex: 1 }}>
      {/* Current: Hero sphere + glass nav cards */}
      <div style={{
        display: "flex", flexDirection: "column", alignItems: "center",
        gap: 12, marginBottom: 24,
      }}>
        <div style={{
          width: 72, height: 72, borderRadius: "50%",
          background: `radial-gradient(circle at 35% 35%, ${f.primary}ee, ${f.primary}88, #111)`,
          boxShadow: `0 0 30px ${f.primary}33, 0 0 60px ${f.primary}11`,
        }} />
        <div style={{
          padding: "8px 18px", borderRadius: 16,
          background: `linear-gradient(135deg, ${t.surfaceElevated}cc, ${t.surface}88)`,
          border: `1px solid ${f.primary}33`,
          backdropFilter: "blur(10px)",
          fontSize: 14, color: t.labelSecondary,
        }}>
          ✓ 2 API keys active
        </div>
      </div>
      {/* Grid of glass nav cards */}
      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10 }}>
        {[
          { icon: "🔑", label: "API Keys", desc: "Provider credentials" },
          { icon: "🤖", label: "Models", desc: "LLM preferences" },
          { icon: "🔊", label: "Sound", desc: "Voice & haptics" },
          { icon: "🛡", label: "Trust", desc: "Autonomy level" },
          { icon: "📱", label: "Phone", desc: "Device control" },
          { icon: "🎨", label: "Appearance", desc: "Theme & flavor" },
        ].map((item, i) => (
          <div key={i} style={{
            padding: "16px 14px", borderRadius: 16,
            background: `linear-gradient(135deg, ${t.surfaceElevated}cc, ${t.surface}88)`,
            border: `1px solid ${t.separator}`,
            backdropFilter: "blur(10px)",
            cursor: "pointer", position: "relative", overflow: "hidden",
          }}>
            <div style={{
              position: "absolute", inset: 0,
              background: `linear-gradient(180deg, rgba(255,255,255,0.04) 0%, transparent 50%)`,
              pointerEvents: "none",
            }} />
            <div style={{ fontSize: 22, marginBottom: 6 }}>{item.icon}</div>
            <div style={{ fontSize: 14, fontWeight: 600, color: t.label, marginBottom: 2 }}>{item.label}</div>
            <div style={{ fontSize: 12, color: t.labelTertiary }}>{item.desc}</div>
          </div>
        ))}
      </div>
      <div style={{
        marginTop: 16, padding: 14, borderRadius: 14, textAlign: "center",
        background: `${t.surfaceElevated}88`, border: `1px solid ${t.separator}`,
        fontSize: 13, color: t.labelTertiary,
      }}>
        About Citros · v0.1
      </div>
    </div>
  );

  const ProposedSettingsHub = () => (
    <div style={{ overflowY: "auto", flex: 1, padding: "8px 0" }}>
      {/* Profile header */}
      <div style={{
        display: "flex", alignItems: "center", gap: 14,
        padding: "12px 16px 20px",
      }}>
        <div style={{
          width: 56, height: 56, borderRadius: "50%",
          background: f.primary, display: "flex", alignItems: "center", justifyContent: "center",
        }}>
          <div style={{ width: 26, height: 26, borderRadius: "50%", background: "rgba(0,0,0,0.15)" }} />
        </div>
        <div>
          <div style={{ fontSize: 22, fontWeight: 700, letterSpacing: -0.5 }}>Citros</div>
          <div style={{ fontSize: 14, color: t.labelSecondary }}>2 providers connected</div>
        </div>
      </div>

      <GroupedSection title="General">
        <GroupedRow label="Appearance" detail={f.label + " · Dark"} showChevron onClick={() => setSubPage("appearance")} />
        <GroupedRow label="Models" detail="Claude Sonnet 4.5" showChevron onClick={() => setSubPage("models")} />
        <GroupedRow label="Sound & Haptics" showChevron onClick={() => setSubPage("sound")} showSep={false} />
      </GroupedSection>

      <GroupedSection title="Privacy & Control">
        <GroupedRow
          label="Trust Level"
          trailing={<Badge text="Ask for risky" color={t.systemOrange} />}
          showChevron onClick={() => setSubPage("trust")}
        />
        <GroupedRow
          label="Phone Control"
          trailing={<Badge text="Active" color={t.systemGreen} />}
          showChevron onClick={() => setSubPage("phone")}
          showSep={false}
        />
      </GroupedSection>

      <GroupedSection title="Account">
        <GroupedRow label="API Keys" detail="Anthropic, OpenAI" showChevron />
        <GroupedRow label="About" detail="v0.1.0" showChevron showSep={false} />
      </GroupedSection>

      <GroupedSection>
        <GroupedRow label="Sign Out" destructive showSep={false} />
      </GroupedSection>
    </div>
  );

  const ProposedAppearancePage = () => (
    <div style={{ overflowY: "auto", flex: 1, padding: "8px 0" }}>
      <GroupedSection title="Flavor">
        <div style={{ padding: "14px 16px", display: "flex", gap: 12 }}>
          {Object.entries(FLAVORS).map(([name, fl]) => (
            <div key={name} onClick={() => setFlavor(name)} style={{
              display: "flex", flexDirection: "column", alignItems: "center", gap: 6, cursor: "pointer",
            }}>
              <div style={{
                width: 44, height: 44, borderRadius: "50%", background: fl.primary,
                border: flavor === name ? `3px solid ${t.label}` : "3px solid transparent",
                transition: "all 0.15s",
              }} />
              <span style={{
                fontSize: 11, color: flavor === name ? t.label : t.labelTertiary,
                fontWeight: flavor === name ? 600 : 400,
              }}>{fl.label}</span>
            </div>
          ))}
        </div>
      </GroupedSection>

      <GroupedSection title="Theme">
        <div style={{ padding: "12px 16px", display: "flex", gap: 10 }}>
          {["Dark", "Light", "System"].map(mode => (
            <div key={mode} style={{
              flex: 1, padding: "28px 0 10px", borderRadius: 10,
              background: mode === "Light" ? "#f5f5f5" : mode === "Dark" ? "#1c1c1e" : `linear-gradient(135deg, #1c1c1e 50%, #f5f5f5 50%)`,
              border: mode === "Dark" ? `2px solid ${f.primary}` : `2px solid ${t.surfaceTertiary}`,
              textAlign: "center", cursor: "pointer",
            }}>
              <div style={{
                fontSize: 12, fontWeight: 500, marginTop: 4,
                color: mode === "Light" ? "#333" : t.label,
              }}>{mode}</div>
            </div>
          ))}
        </div>
      </GroupedSection>

      <GroupedSection title="Auto-clear chat">
        <GroupedRow label="Never" trailing={<span style={{ color: f.primary, fontSize: 15 }}>✓</span>} />
        <GroupedRow label="After 1 hour" />
        <GroupedRow label="After 1 day" />
        <GroupedRow label="After 1 week" showSep={false} />
      </GroupedSection>
    </div>
  );

  const ProposedTrustPage = () => (
    <div style={{ overflowY: "auto", flex: 1, padding: "8px 0" }}>
      <GroupedSection title="Autonomy Level">
        {[
          { label: "Ask before everything", desc: "Confirm every action before Citros takes it", icon: "🔒" },
          { label: "Ask for risky actions", desc: "Auto-approve safe actions, ask for sensitive ones", icon: "⚖️", selected: true },
          { label: "Full autonomy", desc: "Citros acts independently on your behalf", icon: "🚀" },
        ].map((item, i) => (
          <GroupedRow
            key={i}
            label={
              <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <span>{item.icon}</span>
                <span>{item.label}</span>
              </span>
            }
            detail={item.desc}
            trailing={item.selected ? <span style={{ color: f.primary, fontSize: 18 }}>✓</span> : null}
            showSep={i < 2}
          />
        ))}
      </GroupedSection>
      <div style={{
        padding: "0 16px", fontSize: 13, color: t.labelTertiary, lineHeight: 1.5,
      }}>
        Trust level controls how much confirmation Citros requires before taking actions on your phone. Higher autonomy means faster execution but less oversight.
      </div>
    </div>
  );

  const renderSubPage = () => {
    switch (subPage) {
      case "appearance": return <ProposedAppearancePage />;
      case "trust": return <ProposedTrustPage />;
      default: return <ProposedSettingsHub />;
    }
  };

  const subPageTitle = {
    appearance: "Appearance",
    trust: "Trust Level",
    models: "Models",
    sound: "Sound & Haptics",
    phone: "Phone Control",
  };

  return (
    <div style={{
      fontFamily: "-apple-system, SF Pro Display, SF Pro Text, system-ui, sans-serif",
      background: "#111", minHeight: "100vh", display: "flex", flexDirection: "column",
      alignItems: "center", padding: "24px 16px", color: t.label,
    }}>
      {/* Controls */}
      <div style={{ width: "100%", maxWidth: 420, marginBottom: 20 }}>
        <h2 style={{ fontSize: 22, fontWeight: 700, letterSpacing: -0.5, margin: 0, marginBottom: 16 }}>
          Settings Hub Comparison
        </h2>
        <div style={{
          display: "inline-flex", background: t.surfaceElevated,
          borderRadius: 9, padding: 2, gap: 1, marginBottom: 12,
        }}>
          {["current", "proposed"].map(v => (
            <button key={v} onClick={() => { setView(v); setSubPage(null); }} style={{
              padding: "6px 14px", borderRadius: 7, border: "none", cursor: "pointer",
              fontSize: 13, fontWeight: 500, transition: "all 0.2s",
              background: view === v ? t.surfaceTertiary : "transparent",
              color: view === v ? t.label : t.labelSecondary,
              textTransform: "capitalize",
            }}>{v}</button>
          ))}
        </div>
      </div>

      {/* Phone frame */}
      <div style={{
        width: phone.width, height: phone.height, borderRadius: 44, overflow: "hidden",
        border: "3px solid #333", background: view === "proposed" ? t.bg : t.bg,
        position: "relative", display: "flex", flexDirection: "column",
        boxShadow: "0 20px 60px rgba(0,0,0,0.5)",
      }}>
        {/* Status bar */}
        <div style={{
          height: 54, padding: "14px 24px 0", display: "flex", justifyContent: "space-between",
          alignItems: "center", fontSize: 14, fontWeight: 600, flexShrink: 0,
        }}>
          <span>9:41</span>
          <div style={{ display: "flex", gap: 5, alignItems: "center" }}>
            <div style={{ width: 17, height: 11, border: `1px solid ${t.label}`, borderRadius: 2, position: "relative" }}>
              <div style={{ position: "absolute", inset: 1.5, borderRadius: 0.5, background: t.label }} />
            </div>
          </div>
        </div>

        {/* Nav bar */}
        {view === "proposed" ? (
          <div style={{
            padding: "2px 16px 10px", flexShrink: 0,
            display: "flex", alignItems: "center",
          }}>
            {subPage ? (
              <>
                <button onClick={() => setSubPage(null)} style={{
                  background: "none", border: "none", color: f.primary,
                  fontSize: 16, cursor: "pointer", padding: "4px 0",
                  display: "flex", alignItems: "center", gap: 4,
                }}>
                  <svg width="10" height="16" viewBox="0 0 10 16" fill="none">
                    <path d="M9 1L2 8l7 7" stroke={f.primary} strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
                  </svg>
                  Settings
                </button>
                <div style={{ flex: 1, textAlign: "center" }}>
                  <span style={{ fontSize: 17, fontWeight: 600, letterSpacing: -0.3 }}>
                    {subPageTitle[subPage] || "Settings"}
                  </span>
                </div>
                <div style={{ width: 70 }} />
              </>
            ) : (
              <span style={{ fontSize: 34, fontWeight: 700, letterSpacing: -0.8 }}>Settings</span>
            )}
          </div>
        ) : (
          <div style={{
            padding: "8px 16px 12px", flexShrink: 0,
            display: "flex", alignItems: "center", gap: 8,
          }}>
            <svg width="10" height="16" viewBox="0 0 10 16" fill="none">
              <path d="M9 1L2 8l7 7" stroke={f.primary} strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
            <span style={{ fontSize: 17, fontWeight: 600 }}>Settings</span>
          </div>
        )}

        {/* Content */}
        {view === "proposed" ? renderSubPage() : <CurrentSettingsHub />}

        {/* Home indicator */}
        <div style={{
          height: 34, display: "flex", justifyContent: "center", alignItems: "center", flexShrink: 0,
        }}>
          <div style={{ width: 134, height: 5, borderRadius: 3, background: t.surfaceTertiary }} />
        </div>
      </div>

      {/* Design notes */}
      <div style={{
        maxWidth: 420, width: "100%", marginTop: 24, padding: 20,
        background: t.surface, borderRadius: 16, fontSize: 13,
        color: t.labelSecondary, lineHeight: 1.6,
      }}>
        <div style={{ fontWeight: 600, color: t.label, marginBottom: 8, fontSize: 15 }}>
          {view === "current" ? "Current: Glass Card Grid" : "Proposed: iOS Grouped Lists"}
        </div>
        {view === "current" ? (
          <p style={{ margin: 0 }}>
            The current settings hub uses a 2-column grid of glass-morphism cards with a hero sphere at the top. Each card has layered gradients, borders, and blur effects. While visually cohesive with the chat screen, it makes settings feel like a dashboard rather than a utility — settings should feel fast, scannable, and invisible.
          </p>
        ) : (
          <ul style={{ margin: 0, paddingLeft: 16 }}>
            <li style={{ marginBottom: 6 }}>iOS-style grouped table views with inset rounded sections</li>
            <li style={{ marginBottom: 6 }}>Large title navigation (34px) with inline back button</li>
            <li style={{ marginBottom: 6 }}>Status badges (Active, Ask for risky) provide info at-a-glance</li>
            <li style={{ marginBottom: 6 }}>Appearance page: visual theme previews instead of text-only pills</li>
            <li style={{ marginBottom: 6 }}>Destructive actions (Sign Out) isolated in own section per HIG</li>
            <li>Click "Appearance" or "Trust Level" rows to see sub-pages</li>
          </ul>
        )}
      </div>
    </div>
  );
}