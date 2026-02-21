import { useState } from "react";

/*
 * DIRECTIVE A — Material You / Dynamic Color — Settings
 *
 * Every surface is tonal-palette-derived. No "none" flavor.
 * 8dp grid, Google Sans metrics, M3 list items + switches.
 * Settings uses surfaceContainerHigh for grouped cards.
 */

const GRID = 8;
const g = (n) => n * GRID;

const palettes = {
  lemon: {
    primary: "#FFD600", onPrimary: "#3A3000",
    primaryContainer: "#FFF1A0", onPrimaryContainer: "#221B00",
    secondary: "#6D6740", onSecondary: "#FFFFFF",
    surface: "#1E1B16", surfaceBright: "#2D2A22", surfaceContainer: "#252218",
    surfaceContainerHigh: "#302D25", surfaceContainerHighest: "#3B382F",
    onSurface: "#ECE6D4", onSurfaceVariant: "#CDC6B1",
    outline: "#968F7E", outlineVariant: "#4A4639",
    error: "#FFB4AB", onError: "#690005",
    surfaceLight: "#FFF8E1", surfaceBrightLight: "#FFFFFF",
    surfaceContainerLight: "#FFF3CC", surfaceContainerHighLight: "#FFECB0",
    surfaceContainerHighestLight: "#FFE59D",
    onSurfaceLight: "#1E1B16", onSurfaceVariantLight: "#4A4639",
    outlineLight: "#7C7768", outlineVariantLight: "#CDC6B1",
    errorLight: "#BA1A1A", onErrorLight: "#FFFFFF",
  },
  tangerine: {
    primary: "#FF8C00", onPrimary: "#FFFFFF",
    primaryContainer: "#FFD5A0", onPrimaryContainer: "#2B1700",
    secondary: "#6B5D3F", onSecondary: "#FFFFFF",
    surface: "#1F1B16", surfaceBright: "#2E2921", surfaceContainer: "#262118",
    surfaceContainerHigh: "#312B22", surfaceContainerHighest: "#3C362C",
    onSurface: "#EDE4D4", onSurfaceVariant: "#D0C4AE",
    outline: "#998D7A", outlineVariant: "#4D4336",
    error: "#FFB4AB", onError: "#690005",
    surfaceLight: "#FFF3E0", surfaceBrightLight: "#FFFFFF",
    surfaceContainerLight: "#FFE8CC", surfaceContainerHighLight: "#FFDCB0",
    surfaceContainerHighestLight: "#FFD09A",
    onSurfaceLight: "#1F1B16", onSurfaceVariantLight: "#4D4336",
    outlineLight: "#7F7567", outlineVariantLight: "#D0C4AE",
    errorLight: "#BA1A1A", onErrorLight: "#FFFFFF",
  },
  lime: {
    primary: "#7CB342", onPrimary: "#FFFFFF",
    primaryContainer: "#C5E99B", onPrimaryContainer: "#0F2000",
    secondary: "#586249", onSecondary: "#FFFFFF",
    surface: "#1A1E15", surfaceBright: "#272C20", surfaceContainer: "#21261A",
    surfaceContainerHigh: "#2C3124", surfaceContainerHighest: "#373C2F",
    onSurface: "#DEE7CC", onSurfaceVariant: "#C1CAA9",
    outline: "#8B9478", outlineVariant: "#434B38",
    error: "#FFB4AB", onError: "#690005",
    surfaceLight: "#F1F8E9", surfaceBrightLight: "#FFFFFF",
    surfaceContainerLight: "#E2F0D0", surfaceContainerHighLight: "#D4E8B8",
    surfaceContainerHighestLight: "#C5E09F",
    onSurfaceLight: "#1A1E15", onSurfaceVariantLight: "#434B38",
    outlineLight: "#6F7862", outlineVariantLight: "#C1CAA9",
    errorLight: "#BA1A1A", onErrorLight: "#FFFFFF",
  },
  blood_orange: {
    primary: "#D84315", onPrimary: "#FFFFFF",
    primaryContainer: "#FFAB91", onPrimaryContainer: "#2C0800",
    secondary: "#6B5B52", onSecondary: "#FFFFFF",
    surface: "#201A17", surfaceBright: "#302824", surfaceContainer: "#27201C",
    surfaceContainerHigh: "#322A26", surfaceContainerHighest: "#3D3531",
    onSurface: "#EDE0DA", onSurfaceVariant: "#D6C3BA",
    outline: "#9E8D84", outlineVariant: "#4F413A",
    error: "#FFB4AB", onError: "#690005",
    surfaceLight: "#FBE9E7", surfaceBrightLight: "#FFFFFF",
    surfaceContainerLight: "#FFDDD2", surfaceContainerHighLight: "#FFD0C0",
    surfaceContainerHighestLight: "#FFC4AD",
    onSurfaceLight: "#201A17", onSurfaceVariantLight: "#4F413A",
    outlineLight: "#827268", outlineVariantLight: "#D6C3BA",
    errorLight: "#BA1A1A", onErrorLight: "#FFFFFF",
  },
  grapefruit: {
    primary: "#E91E63", onPrimary: "#FFFFFF",
    primaryContainer: "#FFB2C8", onPrimaryContainer: "#3A001D",
    secondary: "#6B5258", onSecondary: "#FFFFFF",
    surface: "#201A1C", surfaceBright: "#302729", surfaceContainer: "#272022",
    surfaceContainerHigh: "#322A2C", surfaceContainerHighest: "#3D3537",
    onSurface: "#EDE0E3", onSurfaceVariant: "#D6C1C7",
    outline: "#9E8B92", outlineVariant: "#4F3F44",
    error: "#FFB4AB", onError: "#690005",
    surfaceLight: "#FCE4EC", surfaceBrightLight: "#FFFFFF",
    surfaceContainerLight: "#FFD6E0", surfaceContainerHighLight: "#FFC8D4",
    surfaceContainerHighestLight: "#FFBAC8",
    onSurfaceLight: "#201A1C", onSurfaceVariantLight: "#4F3F44",
    outlineLight: "#827078", outlineVariantLight: "#D6C1C7",
    errorLight: "#BA1A1A", onErrorLight: "#FFFFFF",
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
    error: p.error, onError: p.onError,
  };
  return {
    bg: p.surfaceLight, surface: p.surfaceContainerLight, surfaceHigh: p.surfaceContainerHighLight,
    surfaceHighest: p.surfaceContainerHighestLight, bright: p.surfaceBrightLight,
    primary: p.primary, onPrimary: p.onPrimary,
    primaryContainer: p.primaryContainer, onPrimaryContainer: p.onPrimaryContainer,
    onSurface: p.onSurfaceLight, onSurfaceVariant: p.onSurfaceVariantLight,
    outline: p.outlineLight, outlineVariant: p.outlineVariantLight,
    error: p.errorLight, onError: p.onErrorLight,
  };
};

/* ── Icons (24×24, 1.5 stroke, round) ── */
const BackIcon = (c) => <svg width="24" height="24" viewBox="0 0 24 24" fill="none"><path d="M15 6l-6 6 6 6" stroke={c} strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"/></svg>;
const CheckIcon = (c) => <svg width="18" height="18" viewBox="0 0 18 18" fill="none"><path d="M4 9.5L7.5 13L14 5" stroke={c} strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"/></svg>;

/* ── M3 Switch ── */
const M3Switch = ({ on, m }) => (
  <div style={{
    width: 52, height: 32, borderRadius: 16, cursor: "pointer",
    background: on ? m.primary : m.surfaceHighest,
    border: on ? "none" : `2px solid ${m.outline}`,
    padding: on ? 2 : 0, transition: "all 200ms cubic-bezier(0.2,0,0,1)",
    display: "flex", alignItems: "center",
  }}>
    <div style={{
      width: on ? 24 : 16, height: on ? 24 : 16,
      borderRadius: on ? 12 : 8,
      background: on ? m.onPrimary : m.outline,
      transform: on ? "translateX(22px)" : "translateX(6px)",
      transition: "all 200ms cubic-bezier(0.2,0,0,1)",
      display: "grid", placeItems: "center",
    }}>
      {on && <svg width="12" height="12" viewBox="0 0 12 12" fill="none"><path d="M2 6.5L5 9.5l5-7" stroke={m.primary} strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"/></svg>}
    </div>
  </div>
);

/* ── Orb ── */
const Orb = ({ color, size = 40 }) => (
  <div style={{ width: size, height: size, borderRadius: size / 2, background: color, display: "grid", placeItems: "center", flexShrink: 0 }}>
    <div style={{ width: size * 0.38, height: size * 0.38, borderRadius: size * 0.19, background: "rgba(0,0,0,0.15)" }} />
  </div>
);

/* ── M3 List Items ── */
const Section = ({ title, children, m }) => (
  <div style={{ marginBottom: g(1) }}>
    {title && <div style={{ fontSize: 14, fontWeight: 500, color: m.primary, letterSpacing: 0.1, paddingLeft: g(2), marginBottom: g(1), lineHeight: "20px" }}>{title}</div>}
    <div style={{ background: m.surfaceHigh, borderRadius: 28, overflow: "hidden", marginInline: g(2) }}>{children}</div>
  </div>
);

const ListItem = ({ headline, supporting, trailing, chevron, sep = true, destructive, m, onClick }) => (
  <div onClick={onClick} style={{
    display: "flex", alignItems: "center", padding: `${g(1)}px ${g(3)}px ${g(1)}px ${g(2)}px`,
    borderBottom: sep ? `1px solid ${m.outlineVariant}` : "none",
    cursor: onClick ? "pointer" : "default", minHeight: g(7),
  }}>
    <div style={{ flex: 1, minWidth: 0, padding: `${g(1)}px 0` }}>
      <div style={{ fontSize: 16, letterSpacing: 0.15, color: destructive ? m.error : m.onSurface, lineHeight: "24px" }}>{headline}</div>
      {supporting && <div style={{ fontSize: 14, color: m.onSurfaceVariant, letterSpacing: 0.25, lineHeight: "20px" }}>{supporting}</div>}
    </div>
    {trailing}
    {chevron && <svg width="24" height="24" viewBox="0 0 24 24" fill="none" style={{ marginLeft: g(1), opacity: 0.5 }}><path d="M9 6l6 6-6 6" stroke={m.onSurfaceVariant} strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/></svg>}
  </div>
);

const Badge = ({ text, color, m }) => <span style={{ fontSize: 12, fontWeight: 500, padding: `4px ${g(1.5)}px`, borderRadius: 8, background: `${color}20`, color, letterSpacing: 0.4 }}>{text}</span>;

/* ── Sub-page ── */
const SubPage = ({ title, onBack, children, m }) => (
  <div style={{ display: "flex", flexDirection: "column", flex: 1 }}>
    <div style={{ display: "flex", alignItems: "center", padding: `${g(1)}px ${g(0.5)}px`, height: g(8), flexShrink: 0 }}>
      <button onClick={onBack} style={{ width: 48, height: 48, display: "grid", placeItems: "center", background: "none", border: "none", cursor: "pointer" }}>{BackIcon(m.onSurface)}</button>
      <div style={{ fontSize: 22, fontWeight: 400, letterSpacing: 0, color: m.onSurface }}>{title}</div>
    </div>
    <div style={{ flex: 1, overflowY: "auto", paddingTop: g(1) }}>{children}</div>
  </div>
);

/* ── Pages ── */
const HubPage = ({ m, flavor, navigate }) => (
  <div style={{ flex: 1, overflowY: "auto" }}>
    <div style={{ display: "flex", alignItems: "center", gap: g(2), padding: `${g(2)}px ${g(2)}px ${g(3)}px` }}>
      <Orb color={m.primary} size={g(7)} />
      <div>
        <div style={{ fontSize: 22, fontWeight: 400, color: m.onSurface }}>Citros</div>
        <div style={{ fontSize: 14, color: m.onSurfaceVariant, letterSpacing: 0.25 }}>2 providers connected</div>
      </div>
    </div>
    <Section title="General" m={m}>
      <ListItem m={m} headline="Appearance" supporting={`${palettes[flavor] ? flavor.charAt(0).toUpperCase() + flavor.slice(1).replace("_"," ") : flavor}`} chevron onClick={() => navigate("appearance")} />
      <ListItem m={m} headline="Models" supporting="Claude Sonnet 4.5" chevron onClick={() => navigate("models")} />
      <ListItem m={m} headline="Sound & Haptics" chevron sep={false} onClick={() => navigate("sound")} />
    </Section>
    <Section title="Privacy & Control" m={m}>
      <ListItem m={m} headline="Trust Level" trailing={<Badge text="Ask for risky" color={m.primary} m={m} />} chevron onClick={() => navigate("trust")} />
      <ListItem m={m} headline="Phone Control" trailing={<Badge text="Active" color="#4CAF50" m={m} />} chevron sep={false} onClick={() => navigate("phone")} />
    </Section>
    <Section title="Account" m={m}>
      <ListItem m={m} headline="API Keys" supporting="Anthropic, OpenAI" chevron onClick={() => navigate("keys")} />
      <ListItem m={m} headline="About" supporting="v0.1.0" chevron sep={false} onClick={() => navigate("about")} />
    </Section>
    <div style={{ padding: `0 ${g(2)}px`, marginTop: g(0.5) }}>
      <div style={{ background: m.surfaceHigh, borderRadius: 28, overflow: "hidden" }}>
        <ListItem m={m} headline="Sign Out" destructive sep={false} />
      </div>
    </div>
  </div>
);

const AppearancePage = ({ m, flavor, setFlavor, theme, setTheme, onBack }) => (
  <SubPage title="Appearance" onBack={onBack} m={m}>
    <Section title="Palette" m={m}>
      <div style={{ padding: `${g(2)}px ${g(3)}px`, display: "flex", gap: g(2), flexWrap: "wrap" }}>
        {Object.entries(palettes).map(([name, p]) => (
          <div key={name} onClick={() => setFlavor(name)} style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: g(0.75), cursor: "pointer" }}>
            <div style={{
              width: g(6), height: g(6), borderRadius: g(3), background: p.primary,
              border: flavor === name ? `3px solid ${m.onSurface}` : "3px solid transparent",
              outline: flavor === name ? `2px solid ${m.primary}` : "none", outlineOffset: 2,
            }} />
            <span style={{ fontSize: 11, color: flavor === name ? m.onSurface : m.onSurfaceVariant, fontWeight: flavor === name ? 500 : 400, letterSpacing: 0.5, textTransform: "capitalize" }}>{name.replace("_", " ")}</span>
          </div>
        ))}
      </div>
    </Section>
    <Section title="Theme" m={m}>
      <div style={{ padding: `${g(2)}px ${g(3)}px`, display: "flex", gap: g(2) }}>
        {[["Dark", "dark", m.bg], ["Light", "light", "#F5F5F0"]].map(([label, val, bg]) => (
          <div key={val} onClick={() => setTheme(val)} style={{ flex: 1, cursor: "pointer" }}>
            <div style={{
              height: g(8), borderRadius: 16,
              background: bg, border: theme === val ? `2px solid ${m.primary}` : `2px solid ${m.outlineVariant}`,
              marginBottom: g(1),
            }} />
            <div style={{ fontSize: 14, fontWeight: 500, textAlign: "center", color: theme === val ? m.onSurface : m.onSurfaceVariant, letterSpacing: 0.1 }}>{label}</div>
          </div>
        ))}
      </div>
    </Section>
    <Section title="Auto-clear chat" m={m}>
      {["Never", "After 1 hour", "After 1 day", "After 1 week"].map((opt, i) => (
        <ListItem key={opt} m={m} headline={opt} trailing={i === 0 ? CheckIcon(m.primary) : null} sep={i < 3} />
      ))}
    </Section>
  </SubPage>
);

const ModelsPage = ({ m, onBack }) => {
  const [selected, setSelected] = useState(1);
  const models = [
    { name: "llama.cpp (local)", desc: "On-device, fastest, limited capability" },
    { name: "Claude Sonnet 4.5", desc: "Cloud, balanced speed and capability" },
    { name: "Claude Opus 4.5", desc: "Cloud, most capable, higher latency" },
  ];
  return (
    <SubPage title="Models" onBack={onBack} m={m}>
      <Section title="Default model" m={m}>
        {models.map((mod, i) => (
          <ListItem key={i} m={m} headline={mod.name} supporting={mod.desc} sep={i < 2}
            trailing={selected === i ? CheckIcon(m.primary) : null} onClick={() => setSelected(i)} />
        ))}
      </Section>
      <Section title="Fallback" m={m}>
        <ListItem m={m} headline="Use local model when offline" trailing={<M3Switch on={true} m={m} />} sep={false} />
      </Section>
    </SubPage>
  );
};

const TrustPage = ({ m, onBack }) => {
  const [level, setLevel] = useState(1);
  const items = [
    { label: "Ask before everything", desc: "Confirm every action before Citros takes it" },
    { label: "Ask for risky actions", desc: "Auto-approve safe, ask for sensitive" },
    { label: "Full autonomy", desc: "Citros acts independently on your behalf" },
  ];
  return (
    <SubPage title="Trust Level" onBack={onBack} m={m}>
      <Section title="Autonomy Level" m={m}>
        {items.map((item, i) => (
          <ListItem key={i} m={m} headline={item.label} supporting={item.desc} sep={i < 2}
            trailing={level === i ? CheckIcon(m.primary) : null} onClick={() => setLevel(i)} />
        ))}
      </Section>
      <div style={{ padding: `${g(1)}px ${g(4)}px`, fontSize: 14, color: m.onSurfaceVariant, lineHeight: "20px", letterSpacing: 0.25 }}>
        Controls how much confirmation Citros requires before taking actions on your phone.
      </div>
    </SubPage>
  );
};

const SoundPage = ({ m, onBack }) => (
  <SubPage title="Sound & Haptics" onBack={onBack} m={m}>
    <Section title="Voice" m={m}>
      <ListItem m={m} headline="Read responses aloud" trailing={<M3Switch on={false} m={m} />} />
      <ListItem m={m} headline="Auto-send voice input" supporting="Send when speech stops" trailing={<M3Switch on={true} m={m} />} sep={false} />
    </Section>
    <Section title="Feedback" m={m}>
      <ListItem m={m} headline="Haptic feedback" trailing={<M3Switch on={true} m={m} />} />
      <ListItem m={m} headline="Sound effects" trailing={<M3Switch on={false} m={m} />} sep={false} />
    </Section>
  </SubPage>
);

const PhonePage = ({ m, onBack }) => (
  <SubPage title="Phone Control" onBack={onBack} m={m}>
    <Section title="Permissions" m={m}>
      <ListItem m={m} headline="Accessibility Service" trailing={<Badge text="Granted" color="#4CAF50" m={m} />} chevron />
      <ListItem m={m} headline="Overlay Permission" trailing={<Badge text="Granted" color="#4CAF50" m={m} />} chevron sep={false} />
    </Section>
    <Section title="Default Overlay" m={m}>
      {["Mini Chat", "Bubble", "Dynamic Island"].map((opt, i) => (
        <ListItem key={opt} m={m} headline={opt} sep={i < 2}
          trailing={i === 2 ? CheckIcon(m.primary) : null} />
      ))}
    </Section>
  </SubPage>
);

const KeysPage = ({ m, onBack }) => (
  <SubPage title="API Keys" onBack={onBack} m={m}>
    <Section title="Connected providers" m={m}>
      <ListItem m={m} headline="Anthropic" supporting="sk-ant-•••••4f2a" trailing={<Badge text="Active" color="#4CAF50" m={m} />} chevron />
      <ListItem m={m} headline="OpenAI" supporting="sk-•••••8b7c" trailing={<Badge text="Active" color="#4CAF50" m={m} />} chevron sep={false} />
    </Section>
    <div style={{ padding: `0 ${g(2)}px`, marginTop: g(0.5) }}>
      <div style={{ background: m.surfaceHigh, borderRadius: 28, overflow: "hidden" }}>
        <ListItem m={m} headline="Add provider" trailing={<svg width="24" height="24" viewBox="0 0 24 24" fill="none"><path d="M12 5v14M5 12h14" stroke={m.primary} strokeWidth="2" strokeLinecap="round"/></svg>} sep={false} />
      </div>
    </div>
  </SubPage>
);

const AboutPage = ({ m, onBack }) => (
  <SubPage title="About" onBack={onBack} m={m}>
    <Section m={m}>
      <ListItem m={m} headline="Version" trailing={<span style={{ fontSize: 14, color: m.onSurfaceVariant, letterSpacing: 0.25 }}>0.1.0</span>} />
      <ListItem m={m} headline="Build" trailing={<span style={{ fontSize: 14, color: m.onSurfaceVariant, letterSpacing: 0.25 }}>2026.02.19</span>} />
      <ListItem m={m} headline="Device" trailing={<span style={{ fontSize: 14, color: m.onSurfaceVariant, letterSpacing: 0.25 }}>Pixel 10 Pro</span>} sep={false} />
    </Section>
    <Section m={m}>
      <ListItem m={m} headline="Licenses" chevron />
      <ListItem m={m} headline="Privacy Policy" chevron />
      <ListItem m={m} headline="Source Code" chevron sep={false} />
    </Section>
  </SubPage>
);

/* ── Controls ── */
const Controls = ({ flavor, setFlavor, theme, setTheme }) => (
  <div style={{ position: "fixed", top: g(1), right: g(1), zIndex: 10 }}>
    <div style={{ background: "#1C1C1E", borderRadius: 12, padding: g(1.5), display: "flex", flexDirection: "column", gap: g(1.5), color: "#fff" }}>
      <div>
        <div style={{ fontSize: 11, fontWeight: 600, color: "rgba(235,235,245,0.30)", letterSpacing: 0.5, textTransform: "uppercase", marginBottom: g(1) }}>Theme</div>
        <div style={{ display: "flex", gap: 6 }}>
          {["dark", "light"].map(th => (
            <button key={th} onClick={() => setTheme(th)} style={{
              padding: "6px 12px", borderRadius: 8, background: theme === th ? "#3A3A3C" : "transparent",
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
    </div>
  </div>
);

/* ── Main ── */
export default function MaterialYouSettings() {
  const [flavor, setFlavor] = useState("tangerine");
  const [theme, setTheme] = useState("dark");
  const [page, setPage] = useState(null);
  const isDark = theme === "dark";
  const m = resolveM3(flavor, isDark);

  const pages = { appearance: AppearancePage, trust: TrustPage, models: ModelsPage, sound: SoundPage, phone: PhonePage, keys: KeysPage, about: AboutPage };
  const PageComponent = page ? pages[page] : null;

  return (
    <div style={{
      fontFamily: "'Google Sans', 'Roboto Flex', Roboto, system-ui, sans-serif",
      background: m.bg, width: 393, minHeight: 852,
      display: "flex", flexDirection: "column", margin: "0 auto",
      WebkitFontSmoothing: "antialiased",
      transition: "background 300ms cubic-bezier(0.2, 0, 0, 1)",
    }}>
      <Controls flavor={flavor} setFlavor={setFlavor} theme={theme} setTheme={setTheme} />

      {!page && (
        <div style={{ padding: `${g(2)}px ${g(2)}px ${g(1)}px`, flexShrink: 0 }}>
          <div style={{ fontSize: 28, fontWeight: 400, letterSpacing: 0, color: m.onSurface }}>Settings</div>
        </div>
      )}

      {PageComponent ? (
        <PageComponent m={m} flavor={flavor} setFlavor={setFlavor} theme={theme} setTheme={setTheme} onBack={() => setPage(null)} />
      ) : (
        <HubPage m={m} flavor={flavor} navigate={setPage} />
      )}

      <div style={{ height: g(4), flexShrink: 0 }} />
    </div>
  );
}