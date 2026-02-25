import { useState } from "react";

/*
 * DIRECTIVE C — iOS-Minimal + Orb Presence — Settings
 *
 * Identical to the current settings mock except:
 *  1. Orb gets box-shadow glow everywhere it appears
 *  2. Profile header orb sits over a subtle flavor wash
 *
 * Same opaque surfaces, same iOS grouped tables, same tokens.
 */

const GRID = 4;
const g = (n) => n * GRID;

const themes = {
  dark: {
    bg: "#000000", surface1: "#1C1C1E", surface2: "#2C2C2E", surface3: "#3A3A3C", surface4: "#48484A",
    labelPrimary: "#FFFFFF", labelSecondary: "rgba(235,235,245,0.60)",
    labelTertiary: "rgba(235,235,245,0.30)", labelQuaternary: "rgba(235,235,245,0.18)",
    separator: "rgba(84,84,88,0.36)", separatorLight: "rgba(84,84,88,0.20)",
    green: "#30D158", red: "#FF453A", orange: "#FF9F0A", blue: "#0A84FF",
    noFlavorOrb: "#FFFFFF", noFlavorOrbInner: "rgba(0,0,0,0.12)",
    noFlavorGlow: "rgba(255,255,255,0.06)",
  },
  light: {
    bg: "#FFFFFF", surface1: "#F2F2F7", surface2: "#E5E5EA", surface3: "#D1D1D6", surface4: "#C7C7CC",
    labelPrimary: "#000000", labelSecondary: "rgba(60,60,67,0.60)",
    labelTertiary: "rgba(60,60,67,0.30)", labelQuaternary: "rgba(60,60,67,0.18)",
    separator: "rgba(60,60,67,0.12)", separatorLight: "rgba(60,60,67,0.06)",
    green: "#34C759", red: "#FF3B30", orange: "#FF9500", blue: "#007AFF",
    noFlavorOrb: "#000000", noFlavorOrbInner: "rgba(255,255,255,0.20)",
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

const orbColor = (flavorKey, t) => flavorKey === "none" ? t.noFlavorOrb : FLAVORS[flavorKey].primary;
const orbInner = (flavorKey, t) => flavorKey === "none" ? t.noFlavorOrbInner : "rgba(0,0,0,0.12)";
const orbGlow = (flavorKey, t) => flavorKey === "none" ? t.noFlavorGlow : FLAVORS[flavorKey].glow;
const flavorWash = (flavorKey) => flavorKey === "none" ? null : FLAVORS[flavorKey].wash;
const accent = (flavorKey, t) => flavorKey === "none" ? t.labelSecondary : FLAVORS[flavorKey].primary;

/* ── Icons (20×20, 1.5 stroke) ── */
const ChevronIcon = (c) => <svg width="7" height="12" viewBox="0 0 7 12" fill="none"><path d="M1 1l5 5-5 5" stroke={c} strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/></svg>;
const BackIcon = (c) => <svg width="10" height="16" viewBox="0 0 10 16" fill="none"><path d="M9 1L2 8l7 7" stroke={c} strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"/></svg>;

/* ── Orb — with glow ── */
const Orb = ({ color, inner, glow, size }) => {
  const r = Math.round(size * 0.38);
  return (
    <div style={{
      width: size, height: size, borderRadius: size / 2, background: color,
      display: "grid", placeItems: "center", flexShrink: 0,
      boxShadow: glow ? `0 0 ${Math.round(size * 0.45)}px ${Math.round(size * 0.18)}px ${glow}` : "none",
      transition: "box-shadow 200ms ease",
    }}>
      <div style={{ width: r, height: r, borderRadius: r / 2, background: inner }} />
    </div>
  );
};

/* ── iOS grouped list primitives ── */
const Section = ({ title, children, t }) => (
  <div style={{ marginBottom: g(7) }}>
    {title && <div style={{ fontSize: 13, fontWeight: 400, color: t.labelSecondary, textTransform: "uppercase", letterSpacing: 0.5, paddingLeft: g(4), marginBottom: g(2) }}>{title}</div>}
    <div style={{ background: t.surface1, borderRadius: g(3), overflow: "hidden", marginInline: g(4) }}>{children}</div>
  </div>
);

const Row = ({ label, detail, trailing, chevron, sep = true, destructive, t, onClick }) => (
  <div onClick={onClick} style={{ display: "flex", alignItems: "center", padding: `${g(3)}px ${g(4)}px`, borderBottom: sep ? `0.5px solid ${t.separatorLight}` : "none", cursor: onClick ? "pointer" : "default", minHeight: g(11) }}>
    <div style={{ flex: 1, minWidth: 0 }}>
      <div style={{ fontSize: 16, letterSpacing: -0.2, color: destructive ? t.red : t.labelPrimary }}>{label}</div>
      {detail && <div style={{ fontSize: 13, color: t.labelTertiary, marginTop: 1, letterSpacing: -0.1 }}>{detail}</div>}
    </div>
    {trailing}
    {chevron && <div style={{ marginLeft: g(2), opacity: 0.3 }}>{ChevronIcon(t.labelPrimary)}</div>}
  </div>
);

const Toggle = ({ on, color, t }) => (
  <div style={{ width: 51, height: 31, borderRadius: 16, background: on ? (color || t.green) : t.surface3, padding: 2, cursor: "pointer", transition: "background 0.2s", display: "flex", alignItems: "center" }}>
    <div style={{ width: 27, height: 27, borderRadius: 14, background: "#fff", transform: on ? "translateX(20px)" : "translateX(0)", transition: "transform 0.2s ease", boxShadow: "0 1px 3px rgba(0,0,0,0.3)" }} />
  </div>
);

const Badge = ({ text, color }) => <span style={{ fontSize: 12, fontWeight: 500, padding: `${g(0.75)}px ${g(2)}px`, borderRadius: g(1.5), background: `${color}18`, color }}>{text}</span>;
const CheckMark = ({ color }) => <svg width="14" height="14" viewBox="0 0 14 14" fill="none"><path d="M3 7.5L6 10.5 11 4" stroke={color} strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"/></svg>;

/* ── Sub-page scaffold ── */
const SubPage = ({ title, onBack, children, t, flavor }) => (
  <div style={{ display: "flex", flexDirection: "column", flex: 1 }}>
    <div style={{ padding: `${g(1)}px ${g(4)}px ${g(2.5)}px`, display: "flex", alignItems: "center", flexShrink: 0 }}>
      <button onClick={onBack} style={{ background: "none", border: "none", color: accent(flavor, t), fontSize: 16, cursor: "pointer", padding: `${g(1)}px 0`, display: "flex", alignItems: "center", gap: g(1) }}>
        {BackIcon(accent(flavor, t))}
        <span style={{ letterSpacing: -0.2 }}>Settings</span>
      </button>
      <div style={{ flex: 1, textAlign: "center" }}>
        <span style={{ fontSize: 17, fontWeight: 600, letterSpacing: -0.3, color: t.labelPrimary }}>{title}</span>
      </div>
      <div style={{ width: 70 }} />
    </div>
    <div style={{ flex: 1, overflowY: "auto", paddingTop: g(2) }}>{children}</div>
  </div>
);

/* ── Individual settings pages ── */
const HubPage = ({ t, flavor, setFlavor, navigate }) => {
  const wash = flavorWash(flavor);
  return (
    <div style={{ flex: 1, overflowY: "auto", paddingTop: g(2) }}>
      {/* ▼ DIRECTIVE C: Profile header with subtle flavor wash behind orb */}
      <div style={{
        display: "flex", alignItems: "center", gap: g(3.5), padding: `${g(3)}px ${g(4)}px ${g(5)}px`,
        background: wash
          ? `radial-gradient(ellipse 50% 80% at ${g(4) + g(7)}px 50%, ${wash}, transparent 70%)`
          : "none",
        transition: "background 300ms ease",
      }}>
        <Orb color={orbColor(flavor, t)} inner={orbInner(flavor, t)} glow={orbGlow(flavor, t)} size={g(14)} />
        <div>
          <div style={{ fontSize: 22, fontWeight: 700, letterSpacing: -0.5, color: t.labelPrimary }}>Citros</div>
          <div style={{ fontSize: 14, color: t.labelSecondary, marginTop: 2 }}>2 providers connected</div>
        </div>
      </div>
      <Section title="General" t={t}>
        <Row t={t} label="Appearance" detail={`${FLAVORS[flavor].label} · Dark`} chevron onClick={() => navigate("appearance")} />
        <Row t={t} label="Models" detail="Claude Sonnet 4.5" chevron onClick={() => navigate("models")} />
        <Row t={t} label="Sound & Haptics" chevron sep={false} onClick={() => navigate("sound")} />
      </Section>
      <Section title="Privacy & Control" t={t}>
        <Row t={t} label="Trust Level" trailing={<Badge text="Ask for risky" color={t.orange} />} chevron onClick={() => navigate("trust")} />
        <Row t={t} label="Phone Control" trailing={<Badge text="Active" color={t.green} />} chevron sep={false} onClick={() => navigate("phone")} />
      </Section>
      <Section title="Account" t={t}>
        <Row t={t} label="API Keys" detail="Anthropic, OpenAI" chevron onClick={() => navigate("keys")} />
        <Row t={t} label="About" detail="v0.1.0" chevron sep={false} onClick={() => navigate("about")} />
      </Section>
      <Section t={t}>
        <Row t={t} label="Sign Out" destructive sep={false} />
      </Section>
    </div>
  );
};

const AppearancePage = ({ t, flavor, setFlavor, theme, setTheme, onBack }) => (
  <SubPage title="Appearance" onBack={onBack} t={t} flavor={flavor}>
    <Section title="Flavor" t={t}>
      <div style={{ padding: `${g(3.5)}px ${g(4)}px`, display: "flex", gap: g(3) }}>
        <div onClick={() => setFlavor("none")} style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: g(1.5), cursor: "pointer" }}>
          <div style={{ width: g(11), height: g(11), borderRadius: g(5.5), background: "linear-gradient(135deg, #fff 50%, #000 50%)", border: flavor === "none" ? `3px solid ${t.labelPrimary}` : "3px solid transparent", transition: "all 0.15s" }} />
          <span style={{ fontSize: 11, color: flavor === "none" ? t.labelPrimary : t.labelTertiary, fontWeight: flavor === "none" ? 600 : 400 }}>None</span>
        </div>
        {Object.entries(FLAVORS).filter(([k]) => k !== "none").map(([name, fl]) => (
          <div key={name} onClick={() => setFlavor(name)} style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: g(1.5), cursor: "pointer" }}>
            <div style={{ width: g(11), height: g(11), borderRadius: g(5.5), background: fl.primary, border: flavor === name ? `3px solid ${t.labelPrimary}` : "3px solid transparent", transition: "all 0.15s" }} />
            <span style={{ fontSize: 11, color: flavor === name ? t.labelPrimary : t.labelTertiary, fontWeight: flavor === name ? 600 : 400 }}>{fl.label}</span>
          </div>
        ))}
      </div>
    </Section>
    <Section title="Theme" t={t}>
      <div style={{ padding: `${g(3)}px ${g(4)}px`, display: "flex", gap: g(3) }}>
        {[["Dark", "dark", "#1c1c1e"], ["Light", "light", "#f5f5f5"], ["System", "system", null]].map(([label, val, bg]) => (
          <div key={val} onClick={() => setTheme(val === "system" ? "dark" : val)} style={{ flex: 1, cursor: "pointer" }}>
            <div style={{
              height: g(16), borderRadius: g(2.5),
              background: val === "system" ? "linear-gradient(135deg, #1c1c1e 50%, #f5f5f5 50%)" : bg,
              border: theme === val ? `2px solid ${accent(flavor, t)}` : `2px solid ${t.surface3}`,
              marginBottom: g(2),
            }} />
            <div style={{ fontSize: 13, fontWeight: 500, textAlign: "center", color: theme === val ? t.labelPrimary : t.labelTertiary }}>{label}</div>
          </div>
        ))}
      </div>
    </Section>
    <Section title="Auto-clear chat" t={t}>
      {["Never", "After 1 hour", "After 1 day", "After 1 week"].map((opt, i) => (
        <Row key={opt} t={t} label={opt} trailing={i === 0 ? <CheckMark color={accent(flavor, t)} /> : null} sep={i < 3} />
      ))}
    </Section>
  </SubPage>
);

const TrustPage = ({ t, flavor, onBack }) => {
  const [level, setLevel] = useState(1);
  const items = [
    { label: "Ask before everything", desc: "Confirm every action before Citros takes it" },
    { label: "Ask for risky actions", desc: "Auto-approve safe actions, ask for sensitive ones" },
    { label: "Full autonomy", desc: "Citros acts independently on your behalf" },
  ];
  return (
    <SubPage title="Trust Level" onBack={onBack} t={t} flavor={flavor}>
      <Section title="Autonomy Level" t={t}>
        {items.map((item, i) => (
          <Row key={i} t={t} label={item.label} detail={item.desc} sep={i < 2}
            trailing={level === i ? <CheckMark color={accent(flavor, t)} /> : null}
            onClick={() => setLevel(i)} />
        ))}
      </Section>
      <div style={{ padding: `0 ${g(8)}px`, fontSize: 13, color: t.labelTertiary, lineHeight: "18px", letterSpacing: -0.1 }}>
        Trust level controls how much confirmation Citros requires before taking actions on your phone.
      </div>
    </SubPage>
  );
};

const ModelsPage = ({ t, flavor, onBack }) => {
  const [selected, setSelected] = useState(1);
  const models = [
    { name: "llama.cpp (local)", desc: "On-device, fastest response, limited capability" },
    { name: "Claude Sonnet 4.5", desc: "Cloud, balanced speed and capability" },
    { name: "Claude Opus 4.5", desc: "Cloud, most capable, higher latency" },
  ];
  return (
    <SubPage title="Models" onBack={onBack} t={t} flavor={flavor}>
      <Section title="Default model" t={t}>
        {models.map((m, i) => (
          <Row key={i} t={t} label={m.name} detail={m.desc} sep={i < 2}
            trailing={selected === i ? <CheckMark color={accent(flavor, t)} /> : null}
            onClick={() => setSelected(i)} />
        ))}
      </Section>
      <Section title="Fallback" t={t}>
        <Row t={t} label="Use local model when offline" trailing={<Toggle on={true} t={t} />} sep={false} />
      </Section>
    </SubPage>
  );
};

const SoundPage = ({ t, flavor, onBack }) => (
  <SubPage title="Sound & Haptics" onBack={onBack} t={t} flavor={flavor}>
    <Section title="Voice" t={t}>
      <Row t={t} label="Read responses aloud" trailing={<Toggle on={false} t={t} />} />
      <Row t={t} label="Auto-send voice input" detail="Send message as soon as speech stops" trailing={<Toggle on={true} t={t} />} sep={false} />
    </Section>
    <Section title="Feedback" t={t}>
      <Row t={t} label="Haptic feedback" trailing={<Toggle on={true} t={t} />} />
      <Row t={t} label="Sound effects" trailing={<Toggle on={false} t={t} />} sep={false} />
    </Section>
  </SubPage>
);

const PhonePage = ({ t, flavor, onBack }) => (
  <SubPage title="Phone Control" onBack={onBack} t={t} flavor={flavor}>
    <Section title="Permissions" t={t}>
      <Row t={t} label="Accessibility Service" trailing={<Badge text="Granted" color={t.green} />} chevron />
      <Row t={t} label="Overlay Permission" trailing={<Badge text="Granted" color={t.green} />} chevron sep={false} />
    </Section>
    <Section title="Default Overlay" t={t}>
      {["Mini Chat", "Bubble", "Dynamic Island"].map((opt, i) => (
        <Row key={opt} t={t} label={opt} sep={i < 2}
          trailing={i === 2 ? <CheckMark color={accent(flavor, t)} /> : null} />
      ))}
    </Section>
  </SubPage>
);

const KeysPage = ({ t, flavor, onBack }) => (
  <SubPage title="API Keys" onBack={onBack} t={t} flavor={flavor}>
    <Section title="Connected providers" t={t}>
      <Row t={t} label="Anthropic" detail="sk-ant-•••••4f2a" trailing={<Badge text="Active" color={t.green} />} chevron />
      <Row t={t} label="OpenAI" detail="sk-•••••8b7c" trailing={<Badge text="Active" color={t.green} />} chevron sep={false} />
    </Section>
    <Section t={t}>
      <Row t={t} label="Add provider" trailing={<span style={{ fontSize: 20, color: t.labelTertiary, fontWeight: 300 }}>+</span>} sep={false} />
    </Section>
  </SubPage>
);

const AboutPage = ({ t, flavor, onBack }) => (
  <SubPage title="About" onBack={onBack} t={t} flavor={flavor}>
    <Section t={t}>
      <Row t={t} label="Version" trailing={<span style={{ fontSize: 15, color: t.labelTertiary }}>0.1.0</span>} />
      <Row t={t} label="Build" trailing={<span style={{ fontSize: 15, color: t.labelTertiary }}>2026.02.19</span>} />
      <Row t={t} label="Device" trailing={<span style={{ fontSize: 15, color: t.labelTertiary }}>Pixel 10 Pro</span>} sep={false} />
    </Section>
    <Section t={t}>
      <Row t={t} label="Licenses" chevron />
      <Row t={t} label="Privacy Policy" chevron />
      <Row t={t} label="Source Code" chevron sep={false} />
    </Section>
  </SubPage>
);

/* ── Controls ── */
const Controls = ({ flavor, setFlavor, theme, setTheme }) => {
  const ct = themes.dark;
  return (
    <div style={{ position: "fixed", top: g(4), right: g(4), zIndex: 10 }}>
      <div style={{ background: ct.surface1, borderRadius: g(3), padding: g(3), display: "flex", flexDirection: "column", gap: g(3), color: ct.labelPrimary }}>
        <div>
          <div style={{ fontSize: 11, fontWeight: 600, color: ct.labelTertiary, letterSpacing: 0.5, textTransform: "uppercase", marginBottom: g(2) }}>Theme</div>
          <div style={{ display: "flex", gap: g(1.5) }}>
            {["dark", "light"].map(th => (
              <button key={th} onClick={() => setTheme(th)} style={{ padding: `${g(1.5)}px ${g(3)}px`, borderRadius: g(2), background: theme === th ? ct.surface3 : "transparent", border: "none", fontSize: 12, fontWeight: 500, cursor: "pointer", color: theme === th ? ct.labelPrimary : ct.labelTertiary, textTransform: "capitalize" }}>{th}</button>
            ))}
          </div>
        </div>
        <div>
          <div style={{ fontSize: 11, fontWeight: 600, color: ct.labelTertiary, letterSpacing: 0.5, textTransform: "uppercase", marginBottom: g(2) }}>Flavor</div>
          <div style={{ display: "flex", gap: g(2) }}>
            <button onClick={() => setFlavor("none")} style={{ width: g(7), height: g(7), borderRadius: g(3.5), cursor: "pointer", background: "linear-gradient(135deg, #fff 50%, #000 50%)", border: flavor === "none" ? `2px solid ${ct.labelPrimary}` : "2px solid transparent" }} />
            {Object.entries(FLAVORS).filter(([k]) => k !== "none").map(([n, fl]) => (
              <button key={n} onClick={() => setFlavor(n)} style={{ width: g(7), height: g(7), borderRadius: g(3.5), background: fl.primary, cursor: "pointer", border: flavor === n ? `2px solid ${ct.labelPrimary}` : "2px solid transparent" }} />
            ))}
          </div>
        </div>
      </div>
    </div>
  );
};

/* ── Main ── */
export default function DirectiveCSettings() {
  const [flavor, setFlavor] = useState("tangerine");
  const [theme, setTheme] = useState("dark");
  const [page, setPage] = useState(null);
  const t = themes[theme];
  const ct = themes.dark;

  const pages = { appearance: AppearancePage, trust: TrustPage, models: ModelsPage, sound: SoundPage, phone: PhonePage, keys: KeysPage, about: AboutPage };
  const PageComponent = page ? pages[page] : null;

  return (
    <div style={{ fontFamily: "-apple-system, 'SF Pro Text', system-ui, sans-serif", background: t.bg, width: 393, minHeight: 852, display: "flex", flexDirection: "column", margin: "0 auto", WebkitFontSmoothing: "antialiased", transition: "background 200ms ease" }}>
      <Controls flavor={flavor} setFlavor={setFlavor} theme={theme} setTheme={setTheme} />

      {!page && (
        <div style={{ padding: `${g(2)}px ${g(4)}px ${g(2.5)}px`, flexShrink: 0 }}>
          <div style={{ fontSize: 34, fontWeight: 700, letterSpacing: -0.8, color: t.labelPrimary }}>Settings</div>
        </div>
      )}

      {PageComponent ? (
        <PageComponent t={t} flavor={flavor} setFlavor={setFlavor} theme={theme} setTheme={setTheme} onBack={() => setPage(null)} />
      ) : (
        <HubPage t={t} flavor={flavor} setFlavor={setFlavor} navigate={setPage} />
      )}

      <div style={{ height: g(8.5), display: "flex", justifyContent: "center", alignItems: "center", flexShrink: 0 }}>
        <div style={{ width: 134, height: 5, borderRadius: 3, background: t.surface3 }} />
      </div>
    </div>
  );
}