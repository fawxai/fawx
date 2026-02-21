import { useState } from "react";

const GRID = 4;
const g = (n) => n * GRID;

const themes = {
  dark: {
    bg: "#000000", surface1: "#1C1C1E", surface2: "#2C2C2E", surface3: "#3A3A3C", surface4: "#48484A",
    labelPrimary: "#FFFFFF", labelSecondary: "rgba(235,235,245,0.60)",
    labelTertiary: "rgba(235,235,245,0.30)", labelQuaternary: "rgba(235,235,245,0.18)",
    separator: "rgba(84,84,88,0.36)", green: "#30D158", blue: "#0A84FF",
    noFlavorOrb: "#FFFFFF", noFlavorOrbInner: "rgba(0,0,0,0.12)",
  },
  light: {
    bg: "#FFFFFF", surface1: "#F2F2F7", surface2: "#E5E5EA", surface3: "#D1D1D6", surface4: "#C7C7CC",
    labelPrimary: "#000000", labelSecondary: "rgba(60,60,67,0.60)",
    labelTertiary: "rgba(60,60,67,0.30)", labelQuaternary: "rgba(60,60,67,0.18)",
    separator: "rgba(60,60,67,0.12)", green: "#34C759", blue: "#007AFF",
    noFlavorOrb: "#000000", noFlavorOrbInner: "rgba(255,255,255,0.20)",
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

const orbClr = (fk, t) => fk === "none" ? t.noFlavorOrb : FLAVORS[fk].primary;
const orbInn = (fk, t) => fk === "none" ? t.noFlavorOrbInner : "rgba(0,0,0,0.12)";
const btnBg = (fk, t) => fk === "none" ? t.labelPrimary : FLAVORS[fk].primary;
const btnText = (fk, t) => fk === "none" ? t.bg : (FLAVORS[fk].onPrimary || t.bg);

const Orb = ({ color, inner, size }) => {
  const r = Math.round(size * 0.38);
  return (
    <div style={{ width: size, height: size, borderRadius: size / 2, background: color, display: "grid", placeItems: "center", flexShrink: 0 }}>
      <div style={{ width: r, height: r, borderRadius: r / 2, background: inner }} />
    </div>
  );
};

const CheckIcon = (c) => <svg width="16" height="16" viewBox="0 0 16 16" fill="none"><path d="M3 8.5L6.5 12 13 4" stroke={c} strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"/></svg>;
const ShieldIcon = (c) => <svg width="28" height="28" viewBox="0 0 28 28" fill="none"><path d="M14 3L5 7v6c0 5.5 3.8 10.5 9 12 5.2-1.5 9-6.5 9-12V7l-9-4z" stroke={c} strokeWidth="1.8" strokeLinejoin="round"/><path d="M10 14l2.5 2.5L18 11" stroke={c} strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"/></svg>;
const KeyIcon = (c) => <svg width="28" height="28" viewBox="0 0 28 28" fill="none"><circle cx="10" cy="14" r="4" stroke={c} strokeWidth="1.8"/><path d="M14 14h10M21 11v6M17 11v6" stroke={c} strokeWidth="1.8" strokeLinecap="round"/></svg>;
const PhoneIcon = (c) => <svg width="28" height="28" viewBox="0 0 28 28" fill="none"><rect x="8" y="3" width="12" height="22" rx="2.5" stroke={c} strokeWidth="1.8"/><path d="M12 22h4" stroke={c} strokeWidth="1.8" strokeLinecap="round"/></svg>;

/* ── Step: Welcome ── */
const WelcomeStep = ({ t, flavor, onNext }) => (
  <div style={{ flex: 1, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", padding: `0 ${g(8)}px`, gap: g(5) }}>
    <Orb color={orbClr(flavor, t)} inner={orbInn(flavor, t)} size={g(20)} />
    <div style={{ textAlign: "center" }}>
      <div style={{ fontSize: 28, fontWeight: 700, letterSpacing: -0.8, color: t.labelPrimary, marginBottom: g(2) }}>Citros</div>
      <div style={{ fontSize: 17, color: t.labelSecondary, lineHeight: "24px", letterSpacing: -0.2 }}>
        Your phone, thinking ahead.
      </div>
    </div>
    <button onClick={onNext} style={{ width: "100%", maxWidth: 320, padding: `${g(3.5)}px 0`, borderRadius: g(3), background: btnBg(flavor, t), border: "none", fontSize: 17, fontWeight: 600, letterSpacing: -0.2, color: btnText(flavor, t), cursor: "pointer" }}>
      Get Started
    </button>
  </div>
);

/* ── Step: API Key ── */
const ApiKeyStep = ({ t, flavor, onNext }) => (
  <div style={{ flex: 1, display: "flex", flexDirection: "column", padding: `${g(12)}px ${g(6)}px ${g(6)}px` }}>
    <div style={{ display: "flex", justifyContent: "center", marginBottom: g(6) }}>
      {KeyIcon(orbClr(flavor, t))}
    </div>
    <div style={{ fontSize: 24, fontWeight: 700, letterSpacing: -0.6, color: t.labelPrimary, textAlign: "center", marginBottom: g(2) }}>Connect a Provider</div>
    <div style={{ fontSize: 15, color: t.labelSecondary, textAlign: "center", lineHeight: "22px", letterSpacing: -0.2, marginBottom: g(6) }}>
      Citros needs an API key to connect to a language model.
    </div>
    {/* Provider options */}
    <div style={{ display: "flex", flexDirection: "column", gap: g(2), marginBottom: g(6) }}>
      {[["Anthropic", "Claude models"], ["OpenAI", "GPT models"], ["Local", "On-device llama.cpp"]].map(([name, desc], i) => (
        <div key={i} style={{ display: "flex", alignItems: "center", gap: g(3), padding: `${g(3)}px ${g(4)}px`, borderRadius: g(3), background: t.surface1, cursor: "pointer" }}>
          <div style={{ width: g(10), height: g(10), borderRadius: g(5), background: i === 0 ? orbClr(flavor, t) : t.surface3, display: "grid", placeItems: "center" }}>
            <span style={{ fontSize: 14, fontWeight: 700, color: i === 0 ? (btnText(flavor, t)) : t.labelSecondary }}>{name[0]}</span>
          </div>
          <div style={{ flex: 1 }}>
            <div style={{ fontSize: 16, fontWeight: 500, letterSpacing: -0.2, color: t.labelPrimary }}>{name}</div>
            <div style={{ fontSize: 13, color: t.labelTertiary, letterSpacing: -0.1 }}>{desc}</div>
          </div>
          <div style={{ opacity: 0.3 }}><svg width="7" height="12" viewBox="0 0 7 12" fill="none"><path d="M1 1l5 5-5 5" stroke={t.labelPrimary} strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/></svg></div>
        </div>
      ))}
    </div>
    <div style={{ flex: 1 }} />
    <button onClick={onNext} style={{ width: "100%", padding: `${g(3.5)}px 0`, borderRadius: g(3), background: btnBg(flavor, t), border: "none", fontSize: 17, fontWeight: 600, letterSpacing: -0.2, color: btnText(flavor, t), cursor: "pointer" }}>
      Continue
    </button>
    <button onClick={onNext} style={{ width: "100%", padding: `${g(3)}px 0`, borderRadius: g(3), background: "transparent", border: "none", fontSize: 15, color: t.labelTertiary, cursor: "pointer", marginTop: g(2) }}>
      Skip for now
    </button>
  </div>
);

/* ── Step: Permissions ── */
const PermissionsStep = ({ t, flavor, onNext }) => (
  <div style={{ flex: 1, display: "flex", flexDirection: "column", padding: `${g(12)}px ${g(6)}px ${g(6)}px` }}>
    <div style={{ display: "flex", justifyContent: "center", marginBottom: g(6) }}>
      {ShieldIcon(orbClr(flavor, t))}
    </div>
    <div style={{ fontSize: 24, fontWeight: 700, letterSpacing: -0.6, color: t.labelPrimary, textAlign: "center", marginBottom: g(2) }}>Permissions</div>
    <div style={{ fontSize: 15, color: t.labelSecondary, textAlign: "center", lineHeight: "22px", letterSpacing: -0.2, marginBottom: g(6) }}>
      Citros needs these to act on your behalf.
    </div>
    <div style={{ display: "flex", flexDirection: "column", gap: g(2), marginBottom: g(6) }}>
      {[
        ["Accessibility Service", "Read and interact with screen content", true],
        ["Overlay Permission", "Show Citros over other apps", true],
        ["Notification Access", "Read and manage notifications", false],
      ].map(([name, desc, granted], i) => (
        <div key={i} style={{ display: "flex", alignItems: "center", gap: g(3), padding: `${g(3)}px ${g(4)}px`, borderRadius: g(3), background: t.surface1 }}>
          <div style={{ flex: 1 }}>
            <div style={{ fontSize: 16, fontWeight: 500, letterSpacing: -0.2, color: t.labelPrimary }}>{name}</div>
            <div style={{ fontSize: 13, color: t.labelTertiary, letterSpacing: -0.1 }}>{desc}</div>
          </div>
          {granted ? (
            <div style={{ display: "flex", alignItems: "center", gap: g(1) }}>
              {CheckIcon(t.green)}
              <span style={{ fontSize: 13, color: t.green, fontWeight: 500 }}>Granted</span>
            </div>
          ) : (
            <button style={{ padding: `${g(1.5)}px ${g(3)}px`, borderRadius: g(2), background: t.surface3, border: "none", fontSize: 13, fontWeight: 500, color: t.labelPrimary, cursor: "pointer" }}>Grant</button>
          )}
        </div>
      ))}
    </div>
    <div style={{ flex: 1 }} />
    <button onClick={onNext} style={{ width: "100%", padding: `${g(3.5)}px 0`, borderRadius: g(3), background: btnBg(flavor, t), border: "none", fontSize: 17, fontWeight: 600, letterSpacing: -0.2, color: btnText(flavor, t), cursor: "pointer" }}>
      Continue
    </button>
  </div>
);

/* ── Step: Trust Level ── */
const TrustStep = ({ t, flavor, onNext }) => {
  const [level, setLevel] = useState(1);
  return (
    <div style={{ flex: 1, display: "flex", flexDirection: "column", padding: `${g(12)}px ${g(6)}px ${g(6)}px` }}>
      <div style={{ display: "flex", justifyContent: "center", marginBottom: g(6) }}>
        {PhoneIcon(orbClr(flavor, t))}
      </div>
      <div style={{ fontSize: 24, fontWeight: 700, letterSpacing: -0.6, color: t.labelPrimary, textAlign: "center", marginBottom: g(2) }}>Choose Trust Level</div>
      <div style={{ fontSize: 15, color: t.labelSecondary, textAlign: "center", lineHeight: "22px", letterSpacing: -0.2, marginBottom: g(6) }}>
        How much should Citros ask before acting?
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: g(2), marginBottom: g(6) }}>
        {[
          ["Cautious", "Ask before every action"],
          ["Balanced", "Ask for sensitive actions only"],
          ["Autonomous", "Act independently"],
        ].map(([name, desc], i) => (
          <div key={i} onClick={() => setLevel(i)} style={{
            display: "flex", alignItems: "center", gap: g(3), padding: `${g(3.5)}px ${g(4)}px`, borderRadius: g(3),
            background: t.surface1, cursor: "pointer",
            border: level === i ? `2px solid ${orbClr(flavor, t)}` : `2px solid transparent`,
          }}>
            <div style={{ flex: 1 }}>
              <div style={{ fontSize: 16, fontWeight: 600, letterSpacing: -0.2, color: t.labelPrimary }}>{name}</div>
              <div style={{ fontSize: 13, color: t.labelTertiary, letterSpacing: -0.1 }}>{desc}</div>
            </div>
            {level === i && <CheckIcon color={orbClr(flavor, t)} />}
          </div>
        ))}
      </div>
      <div style={{ flex: 1 }} />
      <button onClick={onNext} style={{ width: "100%", padding: `${g(3.5)}px 0`, borderRadius: g(3), background: btnBg(flavor, t), border: "none", fontSize: 17, fontWeight: 600, letterSpacing: -0.2, color: btnText(flavor, t), cursor: "pointer" }}>
        Finish Setup
      </button>
    </div>
  );
};

/* ── Step: Done ── */
const DoneStep = ({ t, flavor }) => (
  <div style={{ flex: 1, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", padding: `0 ${g(8)}px`, gap: g(5) }}>
    <div style={{ width: g(16), height: g(16), borderRadius: g(8), background: `${orbClr(flavor, t)}15`, display: "grid", placeItems: "center" }}>
      {CheckIcon(orbClr(flavor, t))}
    </div>
    <div style={{ textAlign: "center" }}>
      <div style={{ fontSize: 24, fontWeight: 700, letterSpacing: -0.6, color: t.labelPrimary, marginBottom: g(2) }}>You're all set</div>
      <div style={{ fontSize: 15, color: t.labelSecondary, lineHeight: "22px", letterSpacing: -0.2 }}>
        Citros is ready. Say something or tap the orb to get started.
      </div>
    </div>
  </div>
);

/* ── Main ── */
export default function OnboardingScreen() {
  const [flavor, setFlavor] = useState("tangerine");
  const [theme, setTheme] = useState("dark");
  const [step, setStep] = useState(0);
  const t = themes[theme];
  const ct = themes.dark;

  const steps = [WelcomeStep, ApiKeyStep, PermissionsStep, TrustStep, DoneStep];
  const StepComponent = steps[step];
  const next = () => setStep(s => Math.min(s + 1, steps.length - 1));

  return (
    <div style={{ fontFamily: "-apple-system, 'SF Pro Text', system-ui, sans-serif", background: t.bg, width: 393, minHeight: 852, display: "flex", flexDirection: "column", margin: "0 auto", WebkitFontSmoothing: "antialiased", transition: "background 200ms ease" }}>
      {/* Controls */}
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
          <div>
            <div style={{ fontSize: 11, fontWeight: 600, color: ct.labelTertiary, letterSpacing: 0.5, textTransform: "uppercase", marginBottom: g(2) }}>Step</div>
            <div style={{ display: "flex", gap: g(1.5) }}>
              {["Welcome", "API Key", "Perms", "Trust", "Done"].map((label, i) => (
                <button key={i} onClick={() => setStep(i)} style={{ padding: `${g(1.5)}px ${g(2)}px`, borderRadius: g(2), background: step === i ? ct.surface3 : "transparent", border: "none", fontSize: 11, fontWeight: 500, cursor: "pointer", color: step === i ? ct.labelPrimary : ct.labelTertiary }}>{label}</button>
              ))}
            </div>
          </div>
        </div>
      </div>

      {/* Progress dots */}
      <div style={{ display: "flex", justifyContent: "center", gap: g(2), padding: `${g(4)}px 0`, flexShrink: 0 }}>
        {steps.map((_, i) => (
          <div key={i} style={{
            width: step === i ? g(5) : g(2), height: g(2), borderRadius: g(1),
            background: step === i ? orbClr(flavor, t) : t.surface3,
            transition: "all 200ms ease",
          }} />
        ))}
      </div>

      <StepComponent t={t} flavor={flavor} onNext={next} />

      {/* Home indicator */}
      <div style={{ height: g(8.5), display: "flex", justifyContent: "center", alignItems: "center", flexShrink: 0 }}>
        <div style={{ width: 134, height: 5, borderRadius: 3, background: t.surface3 }} />
      </div>
    </div>
  );
}