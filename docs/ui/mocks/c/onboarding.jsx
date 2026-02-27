import { useState } from "react";

/*
 * DIRECTIVE C — Onboarding (9-step flow)
 *
 * 1. Welcome          — Hero orb with glow + wash
 * 2. Appearance       — Flavor picker + theme (dark/light)
 * 3. Conversation     — Pick a conversation style / personality
 * 4. Getting to Know  — Name, pronouns, interests — so Fawx adapts
 * 5. API Key          — Connect a provider
 * 6. Permissions      — Accessibility, overlay, notifications
 * 7. Trust Level      — Autonomy slider
 * 8. Choose Your Plan — Paywall (free / pro / ultra)
 * 9. Done             — Wash + success
 *
 * Appearance step is live — selecting a flavor/theme updates the
 * rest of the onboarding in real-time, so the user sees their
 * choice reflected immediately.
 */

const GRID = 4;
const g = (n) => n * GRID;

const themes = {
  dark: {
    bg: "#000000", surface1: "#1C1C1E", surface2: "#2C2C2E", surface3: "#3A3A3C", surface4: "#48484A",
    labelPrimary: "#FFFFFF", labelSecondary: "rgba(235,235,245,0.60)",
    labelTertiary: "rgba(235,235,245,0.30)", labelQuaternary: "rgba(235,235,245,0.18)",
    separator: "rgba(84,84,88,0.36)", green: "#30D158", blue: "#0A84FF",
    noFlavorOrb: "#FFFFFF", noFlavorOrbInner: "rgba(0,0,0,0.12)",
    noFlavorGlow: "rgba(255,255,255,0.06)",
  },
  light: {
    bg: "#FFFFFF", surface1: "#F2F2F7", surface2: "#E5E5EA", surface3: "#D1D1D6", surface4: "#C7C7CC",
    labelPrimary: "#000000", labelSecondary: "rgba(60,60,67,0.60)",
    labelTertiary: "rgba(60,60,67,0.30)", labelQuaternary: "rgba(60,60,67,0.18)",
    separator: "rgba(60,60,67,0.12)", green: "#34C759", blue: "#007AFF",
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

const orbClr = (fk, t) => fk === "none" ? t.noFlavorOrb : FLAVORS[fk].primary;
const orbInn = (fk, t) => fk === "none" ? t.noFlavorOrbInner : "rgba(0,0,0,0.12)";
const orbGlw = (fk, t) => fk === "none" ? t.noFlavorGlow : FLAVORS[fk].glow;
const flavorWash = (fk) => fk === "none" ? null : FLAVORS[fk].wash;
const btnBg = (fk, t) => fk === "none" ? t.labelPrimary : FLAVORS[fk].primary;
const btnText = (fk, t) => fk === "none" ? t.bg : (FLAVORS[fk].onPrimary || t.bg);
const accent = (fk, t) => fk === "none" ? t.labelSecondary : FLAVORS[fk].primary;

/* ── Orb with glow ── */
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

/* ── Icons ── */
const CheckIcon = (c) => <svg width="16" height="16" viewBox="0 0 16 16" fill="none"><path d="M3 8.5L6.5 12 13 4" stroke={c} strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"/></svg>;
const ShieldIcon = (c) => <svg width="28" height="28" viewBox="0 0 28 28" fill="none"><path d="M14 3L5 7v6c0 5.5 3.8 10.5 9 12 5.2-1.5 9-6.5 9-12V7l-9-4z" stroke={c} strokeWidth="1.8" strokeLinejoin="round"/><path d="M10 14l2.5 2.5L18 11" stroke={c} strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"/></svg>;
const KeyIcon = (c) => <svg width="28" height="28" viewBox="0 0 28 28" fill="none"><circle cx="10" cy="14" r="4" stroke={c} strokeWidth="1.8"/><path d="M14 14h10M21 11v6M17 11v6" stroke={c} strokeWidth="1.8" strokeLinecap="round"/></svg>;
const PhoneIcon = (c) => <svg width="28" height="28" viewBox="0 0 28 28" fill="none"><rect x="8" y="3" width="12" height="22" rx="2.5" stroke={c} strokeWidth="1.8"/><path d="M12 22h4" stroke={c} strokeWidth="1.8" strokeLinecap="round"/></svg>;
const PaletteIcon = (c) => <svg width="28" height="28" viewBox="0 0 28 28" fill="none"><circle cx="14" cy="14" r="10" stroke={c} strokeWidth="1.8"/><circle cx="14" cy="8.5" r="2" fill={c}/><circle cx="9.5" cy="12" r="2" fill={c}/><circle cx="11" cy="18" r="2" fill={c}/><circle cx="18.5" cy="12" r="2" fill={c}/></svg>;
const ChatBubbleIcon = (c) => <svg width="28" height="28" viewBox="0 0 28 28" fill="none"><path d="M5 6h18a2 2 0 012 2v10a2 2 0 01-2 2H9l-4 3.5V20H5a2 2 0 01-2-2V8a2 2 0 012-2z" stroke={c} strokeWidth="1.8" strokeLinejoin="round"/><path d="M9 12h10M9 15h6" stroke={c} strokeWidth="1.5" strokeLinecap="round"/></svg>;
const UserIcon = (c) => <svg width="28" height="28" viewBox="0 0 28 28" fill="none"><circle cx="14" cy="10" r="4.5" stroke={c} strokeWidth="1.8"/><path d="M6 24c0-4.4 3.6-8 8-8s8 3.6 8 8" stroke={c} strokeWidth="1.8" strokeLinecap="round"/></svg>;
const StarIcon = (c) => <svg width="28" height="28" viewBox="0 0 28 28" fill="none"><path d="M14 4l3 6.2 6.8 1-5 4.8 1.2 6.8L14 19.6 8 22.8l1.2-6.8-5-4.8 6.8-1L14 4z" stroke={c} strokeWidth="1.8" strokeLinejoin="round"/></svg>;
const ChevronIcon = (c) => <svg width="7" height="12" viewBox="0 0 7 12" fill="none"><path d="M1 1l5 5-5 5" stroke={c} strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/></svg>;

/* ═══════════════════════════════════════════════════════
   STEP 1: Welcome
   ═══════════════════════════════════════════════════════ */
const WelcomeStep = ({ t, flavor, onNext }) => {
  const wash = flavorWash(flavor);
  return (
    <div style={{
      flex: 1, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center",
      padding: `0 ${g(8)}px`, gap: g(5),
      background: wash
        ? `radial-gradient(ellipse 60% 40% at 50% 40%, ${wash}, transparent 75%)`
        : "none",
      transition: "background 300ms ease",
    }}>
      <Orb color={orbClr(flavor, t)} inner={orbInn(flavor, t)} glow={orbGlw(flavor, t)} size={g(20)} />
      <div style={{ textAlign: "center" }}>
        <div style={{ fontSize: 28, fontWeight: 700, letterSpacing: -0.8, color: t.labelPrimary, marginBottom: g(2) }}>Fawx</div>
        <div style={{ fontSize: 17, color: t.labelSecondary, lineHeight: "24px", letterSpacing: -0.2 }}>
          Your phone, thinking ahead.
        </div>
      </div>
      <button onClick={onNext} style={{ width: "100%", maxWidth: 320, padding: `${g(3.5)}px 0`, borderRadius: g(3), background: btnBg(flavor, t), border: "none", fontSize: 17, fontWeight: 600, letterSpacing: -0.2, color: btnText(flavor, t), cursor: "pointer" }}>
        Get Started
      </button>
    </div>
  );
};

/* ═══════════════════════════════════════════════════════
   STEP 2: Appearance Choice (NEW)
   ═══════════════════════════════════════════════════════ */
const AppearanceStep = ({ t, flavor, setFlavor, theme, setTheme, onNext }) => (
  <div style={{ flex: 1, display: "flex", flexDirection: "column", padding: `${g(10)}px ${g(6)}px ${g(6)}px` }}>
    <div style={{ display: "flex", justifyContent: "center", marginBottom: g(5) }}>
      {PaletteIcon(orbClr(flavor, t))}
    </div>
    <div style={{ fontSize: 24, fontWeight: 700, letterSpacing: -0.6, color: t.labelPrimary, textAlign: "center", marginBottom: g(2) }}>Make It Yours</div>
    <div style={{ fontSize: 15, color: t.labelSecondary, textAlign: "center", lineHeight: "22px", letterSpacing: -0.2, marginBottom: g(6) }}>
      Pick a flavor and theme. You can change these anytime.
    </div>

    {/* Flavor picker */}
    <div style={{ marginBottom: g(6) }}>
      <div style={{ fontSize: 13, fontWeight: 400, color: t.labelSecondary, textTransform: "uppercase", letterSpacing: 0.5, marginBottom: g(3), textAlign: "center" }}>Flavor</div>
      <div style={{ display: "flex", justifyContent: "center", gap: g(3), flexWrap: "wrap" }}>
        {/* None */}
        <div onClick={() => setFlavor("none")} style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: g(1.5), cursor: "pointer" }}>
          <div style={{ width: g(13), height: g(13), borderRadius: g(6.5), background: "linear-gradient(135deg, #fff 50%, #000 50%)", border: flavor === "none" ? `3px solid ${t.labelPrimary}` : "3px solid transparent", transition: "all 0.15s" }} />
          <span style={{ fontSize: 12, fontWeight: flavor === "none" ? 600 : 400, color: flavor === "none" ? t.labelPrimary : t.labelTertiary }}>None</span>
        </div>
        {Object.entries(FLAVORS).filter(([k]) => k !== "none").map(([name, fl]) => (
          <div key={name} onClick={() => setFlavor(name)} style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: g(1.5), cursor: "pointer" }}>
            <div style={{
              width: g(13), height: g(13), borderRadius: g(6.5), background: fl.primary,
              border: flavor === name ? `3px solid ${t.labelPrimary}` : "3px solid transparent",
              transition: "all 0.15s",
              /* Show glow on selected swatch as a preview */
              boxShadow: flavor === name && fl.glow ? `0 0 16px 6px ${fl.glow}` : "none",
            }} />
            <span style={{ fontSize: 12, fontWeight: flavor === name ? 600 : 400, color: flavor === name ? t.labelPrimary : t.labelTertiary }}>{fl.label}</span>
          </div>
        ))}
      </div>
    </div>

    {/* Theme picker */}
    <div style={{ marginBottom: g(6) }}>
      <div style={{ fontSize: 13, fontWeight: 400, color: t.labelSecondary, textTransform: "uppercase", letterSpacing: 0.5, marginBottom: g(3), textAlign: "center" }}>Theme</div>
      <div style={{ display: "flex", justifyContent: "center", gap: g(3), padding: `0 ${g(4)}px` }}>
        {[["Dark", "dark", "#1c1c1e"], ["Light", "light", "#f5f5f5"]].map(([label, val, bg]) => (
          <div key={val} onClick={() => setTheme(val)} style={{ flex: 1, maxWidth: 140, cursor: "pointer" }}>
            <div style={{
              height: g(20), borderRadius: g(3), background: bg, marginBottom: g(2),
              border: theme === val ? `2px solid ${accent(flavor, t)}` : `2px solid ${t.surface3}`,
              display: "flex", flexDirection: "column", justifyContent: "flex-end", padding: g(2),
              overflow: "hidden", transition: "border-color 0.15s",
            }}>
              {/* Mini preview bubbles */}
              <div style={{ display: "flex", flexDirection: "column", gap: g(1) }}>
                <div style={{ alignSelf: "flex-start", width: "70%", height: g(3), borderRadius: g(2), background: val === "dark" ? "#2C2C2E" : "#E5E5EA" }} />
                <div style={{ alignSelf: "flex-end", width: "50%", height: g(3), borderRadius: g(2), background: orbClr(flavor, themes[val]) }} />
              </div>
            </div>
            <div style={{ fontSize: 14, fontWeight: 500, textAlign: "center", color: theme === val ? t.labelPrimary : t.labelTertiary }}>{label}</div>
          </div>
        ))}
      </div>
    </div>

    <div style={{ flex: 1 }} />
    <button onClick={onNext} style={{ width: "100%", padding: `${g(3.5)}px 0`, borderRadius: g(3), background: btnBg(flavor, t), border: "none", fontSize: 17, fontWeight: 600, letterSpacing: -0.2, color: btnText(flavor, t), cursor: "pointer" }}>
      Continue
    </button>
  </div>
);

/* ═══════════════════════════════════════════════════════
   STEP 3: Conversation Style (NEW)
   ═══════════════════════════════════════════════════════ */
const ConversationStyleStep = ({ t, flavor, onNext }) => {
  const [style, setStyle] = useState(1);
  const styles = [
    {
      name: "Concise",
      desc: "Short and to the point. Fewer follow-ups.",
      preview: "\"Done. Reminder set for 3 PM.\"",
    },
    {
      name: "Balanced",
      desc: "Clear explanations without over-talking.",
      preview: "\"Done — I set a reminder for 3 PM. Want me to add an agenda too?\"",
    },
    {
      name: "Thorough",
      desc: "Detailed reasoning and proactive suggestions.",
      preview: "\"I've set a reminder for 3 PM, 10 minutes before your meeting with Sarah. I noticed there's no agenda attached — should I draft one?\"",
    },
  ];
  return (
    <div style={{ flex: 1, display: "flex", flexDirection: "column", padding: `${g(10)}px ${g(6)}px ${g(6)}px` }}>
      <div style={{ display: "flex", justifyContent: "center", marginBottom: g(5) }}>
        {ChatBubbleIcon(orbClr(flavor, t))}
      </div>
      <div style={{ fontSize: 24, fontWeight: 700, letterSpacing: -0.6, color: t.labelPrimary, textAlign: "center", marginBottom: g(2) }}>How Should I Talk?</div>
      <div style={{ fontSize: 15, color: t.labelSecondary, textAlign: "center", lineHeight: "22px", letterSpacing: -0.2, marginBottom: g(6) }}>
        Choose how Fawx communicates with you.
      </div>

      <div style={{ display: "flex", flexDirection: "column", gap: g(2), marginBottom: g(4) }}>
        {styles.map((s, i) => (
          <div key={i} onClick={() => setStyle(i)} style={{
            padding: `${g(3.5)}px ${g(4)}px`, borderRadius: g(3),
            background: t.surface1, cursor: "pointer",
            border: style === i ? `2px solid ${orbClr(flavor, t)}` : `2px solid transparent`,
            transition: "border-color 0.15s",
          }}>
            <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: g(1.5) }}>
              <div>
                <span style={{ fontSize: 16, fontWeight: 600, letterSpacing: -0.2, color: t.labelPrimary }}>{s.name}</span>
                <span style={{ fontSize: 13, color: t.labelTertiary, marginLeft: g(2), letterSpacing: -0.1 }}>{s.desc}</span>
              </div>
              {style === i && <CheckIcon color={orbClr(flavor, t)} />}
            </div>
            {/* Preview bubble */}
            <div style={{
              padding: `${g(2)}px ${g(3)}px`, borderRadius: `${g(3.5)}px ${g(3.5)}px ${g(3.5)}px ${g(1)}px`,
              background: t.surface2, fontSize: 14, fontStyle: "italic",
              color: t.labelSecondary, lineHeight: "20px", letterSpacing: -0.15,
            }}>
              {s.preview}
            </div>
          </div>
        ))}
      </div>

      <div style={{ flex: 1 }} />
      <button onClick={onNext} style={{ width: "100%", padding: `${g(3.5)}px 0`, borderRadius: g(3), background: btnBg(flavor, t), border: "none", fontSize: 17, fontWeight: 600, letterSpacing: -0.2, color: btnText(flavor, t), cursor: "pointer" }}>
        Continue
      </button>
    </div>
  );
};

/* ═══════════════════════════════════════════════════════
   STEP 4: Getting to Know Each Other (NEW)
   ═══════════════════════════════════════════════════════ */
const KnowYouStep = ({ t, flavor, onNext }) => {
  const [name, setName] = useState("");
  const [selectedInterests, setSelectedInterests] = useState([]);
  const interests = ["Productivity", "Fitness", "Finance", "Travel", "Music", "News", "Shopping", "Cooking", "Work", "Social"];

  const toggleInterest = (item) => {
    setSelectedInterests(prev =>
      prev.includes(item) ? prev.filter(x => x !== item) : [...prev, item]
    );
  };

  return (
    <div style={{ flex: 1, display: "flex", flexDirection: "column", padding: `${g(10)}px ${g(6)}px ${g(6)}px` }}>
      <div style={{ display: "flex", justifyContent: "center", marginBottom: g(5) }}>
        {UserIcon(orbClr(flavor, t))}
      </div>
      <div style={{ fontSize: 24, fontWeight: 700, letterSpacing: -0.6, color: t.labelPrimary, textAlign: "center", marginBottom: g(2) }}>Let's Get Acquainted</div>
      <div style={{ fontSize: 15, color: t.labelSecondary, textAlign: "center", lineHeight: "22px", letterSpacing: -0.2, marginBottom: g(6) }}>
        Help Fawx understand you so it can be more helpful.
      </div>

      {/* Name input */}
      <div style={{ marginBottom: g(5) }}>
        <div style={{ fontSize: 13, fontWeight: 400, color: t.labelSecondary, textTransform: "uppercase", letterSpacing: 0.5, marginBottom: g(2) }}>What should I call you?</div>
        <input
          type="text" value={name} onChange={e => setName(e.target.value)}
          placeholder="Your name"
          style={{
            width: "100%", padding: `${g(3)}px ${g(4)}px`, borderRadius: g(3),
            background: t.surface1, border: "none", outline: "none",
            fontSize: 16, letterSpacing: -0.2, color: t.labelPrimary,
            caretColor: accent(flavor, t), boxSizing: "border-box",
          }}
        />
      </div>

      {/* Interest chips */}
      <div style={{ marginBottom: g(5) }}>
        <div style={{ fontSize: 13, fontWeight: 400, color: t.labelSecondary, textTransform: "uppercase", letterSpacing: 0.5, marginBottom: g(2) }}>I'm interested in</div>
        <div style={{ display: "flex", flexWrap: "wrap", gap: g(2) }}>
          {interests.map(item => {
            const selected = selectedInterests.includes(item);
            return (
              <button key={item} onClick={() => toggleInterest(item)} style={{
                padding: `${g(2)}px ${g(3.5)}px`, borderRadius: g(5),
                background: selected ? (accent(flavor, t) + "18") : t.surface1,
                border: selected ? `1.5px solid ${accent(flavor, t)}` : `1.5px solid transparent`,
                fontSize: 14, fontWeight: selected ? 500 : 400,
                letterSpacing: -0.15, color: selected ? accent(flavor, t) : t.labelSecondary,
                cursor: "pointer", transition: "all 0.15s",
              }}>
                {item}
              </button>
            );
          })}
        </div>
      </div>

      {/* Quick context */}
      <div style={{ padding: `${g(3)}px ${g(4)}px`, borderRadius: g(3), background: t.surface1, marginBottom: g(4) }}>
        <div style={{ fontSize: 14, color: t.labelSecondary, lineHeight: "20px", letterSpacing: -0.15 }}>
          {name ? `"Hey ${name}, ` : `"Hey, `}
          {selectedInterests.length > 0
            ? `I'll keep an eye on ${selectedInterests.slice(0, 2).join(" and ").toLowerCase()}${selectedInterests.length > 2 ? ` and ${selectedInterests.length - 2} more` : ""} for you."`
            : `I'll learn what matters to you as we go."`}
        </div>
        <div style={{ fontSize: 11, color: t.labelTertiary, marginTop: g(1) }}>— How Fawx will greet you</div>
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
};

/* ── Eye toggle icon ── */
const EyeIcon = (c) => <svg width="20" height="20" viewBox="0 0 20 20" fill="none"><ellipse cx="10" cy="10" rx="7.5" ry="4.5" stroke={c} strokeWidth="1.5"/><circle cx="10" cy="10" r="2" stroke={c} strokeWidth="1.5"/></svg>;
const EyeOffIcon = (c) => <svg width="20" height="20" viewBox="0 0 20 20" fill="none"><ellipse cx="10" cy="10" rx="7.5" ry="4.5" stroke={c} strokeWidth="1.5"/><circle cx="10" cy="10" r="2" stroke={c} strokeWidth="1.5"/><line x1="4" y1="16" x2="16" y2="4" stroke={c} strokeWidth="1.5" strokeLinecap="round"/></svg>;
const BackArrowIcon = (c) => <svg width="8" height="14" viewBox="0 0 8 14" fill="none"><path d="M7 1L1 7l6 6" stroke={c} strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/></svg>;

/* ═══════════════════════════════════════════════════════
   STEP 5: Connect a Provider (API Key)
   ═══════════════════════════════════════════════════════ */
const PROVIDERS = [
  { id: "anthropic", label: "Anthropic",   url: "console.anthropic.com/settings/keys" },
  { id: "openrouter", label: "OpenRouter", url: "openrouter.ai/keys" },
  { id: "openai", label: "OpenAI",         url: "platform.openai.com/api-keys" },
];

const ApiKeyStep = ({ t, flavor, onNext, onBack }) => {
  const [provider, setProvider] = useState(0);
  const [apiKey, setApiKey] = useState("");
  const [label, setLabel] = useState("");
  const [showKey, setShowKey] = useState(false);
  const [validated, setValidated] = useState(false);

  const ac = accent(flavor, t);
  const prov = PROVIDERS[provider];

  return (
    <div style={{ flex: 1, display: "flex", flexDirection: "column", padding: `0 ${g(6)}px ${g(6)}px` }}>

      {/* Back link */}
      <button onClick={onBack} style={{
        display: "flex", alignItems: "center", gap: g(1.5),
        background: "none", border: "none", cursor: "pointer",
        padding: `${g(2)}px 0`, marginBottom: g(3), alignSelf: "flex-start",
      }}>
        {BackArrowIcon(ac)}
        <span style={{ fontSize: 17, color: ac, letterSpacing: -0.2 }}>Back</span>
      </button>

      {/* Title */}
      <div style={{ fontSize: 24, fontWeight: 700, letterSpacing: -0.6, color: t.labelPrimary, marginBottom: g(2) }}>
        Connect a Provider
      </div>
      <div style={{ fontSize: 15, color: t.labelSecondary, lineHeight: "22px", letterSpacing: -0.2, marginBottom: g(5) }}>
        Paste your API key to get started.
      </div>

      {/* ── Segmented provider tabs ── */}
      <div style={{
        display: "flex", borderRadius: g(2.5), background: t.surface1,
        padding: g(0.75), marginBottom: g(3),
      }}>
        {PROVIDERS.map((p, i) => {
          const active = provider === i;
          return (
            <button key={p.id} onClick={() => { setProvider(i); setValidated(false); }} style={{
              flex: 1, padding: `${g(2)}px 0`, borderRadius: g(2),
              background: active ? t.surface2 : "transparent",
              border: active ? `1px solid ${t.separator}` : "1px solid transparent",
              fontSize: 14, fontWeight: active ? 600 : 400,
              letterSpacing: -0.15, cursor: "pointer",
              color: active ? t.labelPrimary : t.labelTertiary,
              transition: "all 0.15s",
              boxShadow: active ? "0 1px 3px rgba(0,0,0,0.12)" : "none",
            }}>
              {p.label}
            </button>
          );
        })}
      </div>

      {/* Provider key link */}
      <div style={{ marginBottom: g(5) }}>
        <span style={{ fontSize: 13, color: ac, letterSpacing: -0.1 }}>
          Get a key from {prov.url}
        </span>
      </div>

      {/* ── API Key input ── */}
      <div style={{ marginBottom: g(3) }}>
        <div style={{ fontSize: 13, fontWeight: 400, color: t.labelSecondary, textTransform: "uppercase", letterSpacing: 0.5, marginBottom: g(2) }}>API Key</div>
        <div style={{
          display: "flex", alignItems: "center",
          background: t.surface1, borderRadius: g(3),
          padding: `0 ${g(3)}px`, height: g(12),
        }}>
          <input
            type={showKey ? "text" : "password"}
            value={apiKey}
            onChange={e => { setApiKey(e.target.value); setValidated(false); }}
            placeholder="sk-..."
            style={{
              flex: 1, background: "transparent", border: "none", outline: "none",
              fontSize: 16, letterSpacing: -0.2, color: t.labelPrimary,
              caretColor: ac, fontFamily: "SF Mono, monospace",
              height: "100%",
            }}
          />
          <button onClick={() => setShowKey(!showKey)} style={{
            background: "none", border: "none", cursor: "pointer",
            padding: g(1), display: "grid", placeItems: "center", flexShrink: 0,
          }}>
            {showKey ? EyeOffIcon(t.labelTertiary) : EyeIcon(t.labelTertiary)}
          </button>
        </div>
      </div>

      {/* ── Label input ── */}
      <div style={{ marginBottom: g(5) }}>
        <div style={{ fontSize: 13, fontWeight: 400, color: t.labelSecondary, textTransform: "uppercase", letterSpacing: 0.5, marginBottom: g(2) }}>
          Label <span style={{ textTransform: "none", fontWeight: 400, color: t.labelTertiary }}>(optional)</span>
        </div>
        <div style={{
          display: "flex", alignItems: "center",
          background: t.surface1, borderRadius: g(3),
          padding: `0 ${g(3)}px`, height: g(12),
        }}>
          <input
            type="text"
            value={label}
            onChange={e => setLabel(e.target.value)}
            placeholder="e.g. Personal key"
            style={{
              flex: 1, background: "transparent", border: "none", outline: "none",
              fontSize: 16, letterSpacing: -0.2, color: t.labelPrimary,
              caretColor: ac, height: "100%",
            }}
          />
        </div>
      </div>

      <div style={{ flex: 1 }} />

      {/* ── Validate Key button ── */}
      <button
        onClick={() => setValidated(true)}
        style={{
          width: "100%", padding: `${g(3.5)}px 0`, borderRadius: g(3),
          background: apiKey.length > 0 ? btnBg(flavor, t) : t.surface2,
          border: "none", fontSize: 17, fontWeight: 600, letterSpacing: -0.2,
          color: apiKey.length > 0 ? btnText(flavor, t) : t.labelTertiary,
          cursor: apiKey.length > 0 ? "pointer" : "default",
          transition: "all 0.15s",
          marginBottom: g(2),
        }}
      >
        Validate Key
      </button>

      {/* ── Start Chatting button ── */}
      <button
        onClick={onNext}
        style={{
          width: "100%", padding: `${g(3.5)}px 0`, borderRadius: g(3),
          background: validated ? btnBg(flavor, t) : t.surface2,
          border: "none", fontSize: 17, fontWeight: 600, letterSpacing: -0.2,
          color: validated ? btnText(flavor, t) : t.labelTertiary,
          cursor: validated ? "pointer" : "default",
          transition: "all 0.15s",
        }}
      >
        Start Chatting
      </button>

      {/* ── Skip ── */}
      <button onClick={onNext} style={{
        width: "100%", padding: `${g(3)}px 0`, borderRadius: g(3),
        background: "transparent", border: "none", fontSize: 15,
        color: t.labelTertiary, cursor: "pointer", marginTop: g(2),
      }}>
        Skip for now
      </button>
    </div>
  );
};

/* ═══════════════════════════════════════════════════════
   STEP 6: Permissions
   ═══════════════════════════════════════════════════════ */
const PermissionsStep = ({ t, flavor, onNext }) => (
  <div style={{ flex: 1, display: "flex", flexDirection: "column", padding: `${g(10)}px ${g(6)}px ${g(6)}px` }}>
    <div style={{ display: "flex", justifyContent: "center", marginBottom: g(5) }}>
      {ShieldIcon(orbClr(flavor, t))}
    </div>
    <div style={{ fontSize: 24, fontWeight: 700, letterSpacing: -0.6, color: t.labelPrimary, textAlign: "center", marginBottom: g(2) }}>Permissions</div>
    <div style={{ fontSize: 15, color: t.labelSecondary, textAlign: "center", lineHeight: "22px", letterSpacing: -0.2, marginBottom: g(6) }}>
      Fawx needs these to act on your behalf.
    </div>
    <div style={{ display: "flex", flexDirection: "column", gap: g(2), marginBottom: g(6) }}>
      {[
        ["Accessibility Service", "Read and interact with screen content", true],
        ["Overlay Permission", "Show Fawx over other apps", true],
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

/* ═══════════════════════════════════════════════════════
   STEP 7: Trust Level
   ═══════════════════════════════════════════════════════ */
const TrustStep = ({ t, flavor, onNext }) => {
  const [level, setLevel] = useState(1);
  return (
    <div style={{ flex: 1, display: "flex", flexDirection: "column", padding: `${g(10)}px ${g(6)}px ${g(6)}px` }}>
      <div style={{ display: "flex", justifyContent: "center", marginBottom: g(5) }}>
        {PhoneIcon(orbClr(flavor, t))}
      </div>
      <div style={{ fontSize: 24, fontWeight: 700, letterSpacing: -0.6, color: t.labelPrimary, textAlign: "center", marginBottom: g(2) }}>Choose Trust Level</div>
      <div style={{ fontSize: 15, color: t.labelSecondary, textAlign: "center", lineHeight: "22px", letterSpacing: -0.2, marginBottom: g(6) }}>
        How much should Fawx ask before acting?
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
        Continue
      </button>
    </div>
  );
};

/* ═══════════════════════════════════════════════════════
   STEP 8: Choose Your Plan / Paywall (NEW)
   ═══════════════════════════════════════════════════════ */
const PlanStep = ({ t, flavor, onNext }) => {
  const [plan, setPlan] = useState(1);
  const plans = [
    {
      name: "Free",
      price: "$0",
      period: "forever",
      features: ["5 actions per day", "1 conversation style", "Local model only"],
      cta: "Start Free",
      featured: false,
    },
    {
      name: "Pro",
      price: "$9.99",
      period: "/month",
      features: ["Unlimited actions", "All conversation styles", "Cloud models (Claude, GPT)", "Priority processing"],
      cta: "Start 7-Day Trial",
      featured: true,
    },
    {
      name: "Ultra",
      price: "$19.99",
      period: "/month",
      features: ["Everything in Pro", "Opus-class models", "Custom skills (WASM)", "Phone automation API"],
      cta: "Start 7-Day Trial",
      featured: false,
    },
  ];

  return (
    <div style={{ flex: 1, display: "flex", flexDirection: "column", padding: `${g(10)}px ${g(5)}px ${g(6)}px` }}>
      <div style={{ display: "flex", justifyContent: "center", marginBottom: g(5) }}>
        {StarIcon(orbClr(flavor, t))}
      </div>
      <div style={{ fontSize: 24, fontWeight: 700, letterSpacing: -0.6, color: t.labelPrimary, textAlign: "center", marginBottom: g(2) }}>Choose Your Plan</div>
      <div style={{ fontSize: 15, color: t.labelSecondary, textAlign: "center", lineHeight: "22px", letterSpacing: -0.2, marginBottom: g(5) }}>
        Unlock the full power of Fawx.
      </div>

      <div style={{ display: "flex", flexDirection: "column", gap: g(2.5), marginBottom: g(4) }}>
        {plans.map((p, i) => {
          const selected = plan === i;
          const borderColor = selected ? orbClr(flavor, t) : "transparent";
          return (
            <div key={i} onClick={() => setPlan(i)} style={{
              padding: `${g(3.5)}px ${g(4)}px`, borderRadius: g(3),
              background: t.surface1, cursor: "pointer",
              border: `2px solid ${borderColor}`,
              position: "relative", overflow: "hidden",
              transition: "border-color 0.15s",
            }}>
              {/* Featured badge */}
              {p.featured && (
                <div style={{
                  position: "absolute", top: 0, right: 0,
                  background: orbClr(flavor, t), color: btnText(flavor, t),
                  fontSize: 10, fontWeight: 700, letterSpacing: 0.5, textTransform: "uppercase",
                  padding: `${g(0.75)}px ${g(2.5)}px`,
                  borderRadius: `0 0 0 ${g(2)}px`,
                }}>Popular</div>
              )}

              <div style={{ display: "flex", alignItems: "baseline", gap: g(1.5), marginBottom: g(2) }}>
                <span style={{ fontSize: 18, fontWeight: 700, letterSpacing: -0.4, color: t.labelPrimary }}>{p.name}</span>
                <span style={{ fontSize: 22, fontWeight: 700, letterSpacing: -0.6, color: t.labelPrimary }}>{p.price}</span>
                <span style={{ fontSize: 13, color: t.labelTertiary, letterSpacing: -0.1 }}>{p.period}</span>
                <div style={{ flex: 1 }} />
                {selected && <CheckIcon color={orbClr(flavor, t)} />}
              </div>

              <div style={{ display: "flex", flexDirection: "column", gap: g(1) }}>
                {p.features.map((f, fi) => (
                  <div key={fi} style={{ display: "flex", alignItems: "center", gap: g(2) }}>
                    <svg width="10" height="10" viewBox="0 0 10 10" fill="none"><path d="M2 5.5L4.2 7.5 8 3" stroke={selected ? orbClr(flavor, t) : t.labelTertiary} strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round"/></svg>
                    <span style={{ fontSize: 14, color: t.labelSecondary, letterSpacing: -0.15, lineHeight: "20px" }}>{f}</span>
                  </div>
                ))}
              </div>
            </div>
          );
        })}
      </div>

      <div style={{ flex: 1 }} />

      <button onClick={onNext} style={{
        width: "100%", padding: `${g(3.5)}px 0`, borderRadius: g(3),
        background: btnBg(flavor, t), border: "none",
        fontSize: 17, fontWeight: 600, letterSpacing: -0.2,
        color: btnText(flavor, t), cursor: "pointer",
      }}>
        {plans[plan].cta}
      </button>

      {plan !== 0 && (
        <div style={{ textAlign: "center", marginTop: g(2) }}>
          <span style={{ fontSize: 12, color: t.labelTertiary, letterSpacing: -0.1 }}>
            Cancel anytime. No charge during trial.
          </span>
        </div>
      )}

      <button onClick={onNext} style={{ width: "100%", padding: `${g(3)}px 0`, borderRadius: g(3), background: "transparent", border: "none", fontSize: 15, color: t.labelTertiary, cursor: "pointer", marginTop: g(1.5) }}>
        Maybe later
      </button>
    </div>
  );
};

/* ═══════════════════════════════════════════════════════
   STEP 9: Done
   ═══════════════════════════════════════════════════════ */
const DoneStep = ({ t, flavor }) => {
  const wash = flavorWash(flavor);
  return (
    <div style={{
      flex: 1, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center",
      padding: `0 ${g(8)}px`, gap: g(5),
      background: wash
        ? `radial-gradient(ellipse 60% 40% at 50% 42%, ${wash}, transparent 75%)`
        : "none",
      transition: "background 300ms ease",
    }}>
      <div style={{ width: g(16), height: g(16), borderRadius: g(8), background: `${orbClr(flavor, t)}15`, display: "grid", placeItems: "center" }}>
        {CheckIcon(orbClr(flavor, t))}
      </div>
      <div style={{ textAlign: "center" }}>
        <div style={{ fontSize: 24, fontWeight: 700, letterSpacing: -0.6, color: t.labelPrimary, marginBottom: g(2) }}>You're all set</div>
        <div style={{ fontSize: 15, color: t.labelSecondary, lineHeight: "22px", letterSpacing: -0.2 }}>
          Fawx is ready. Say something or tap the orb to get started.
        </div>
      </div>
    </div>
  );
};

/* ═══════════════════════════════════════════════════════
   Main
   ═══════════════════════════════════════════════════════ */
const STEP_LABELS = ["Welcome", "Look", "Style", "You", "API", "Perms", "Trust", "Plan", "Done"];

export default function OnboardingScreen() {
  const [flavor, setFlavor] = useState("tangerine");
  const [theme, setTheme] = useState("dark");
  const [step, setStep] = useState(0);
  const t = themes[theme];
  const ct = themes.dark;

  const steps = [WelcomeStep, AppearanceStep, ConversationStyleStep, KnowYouStep, ApiKeyStep, PermissionsStep, TrustStep, PlanStep, DoneStep];
  const StepComponent = steps[step];
  const next = () => setStep(s => Math.min(s + 1, steps.length - 1));
  const back = () => setStep(s => Math.max(s - 1, 0));

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
            <div style={{ display: "flex", gap: g(1), flexWrap: "wrap" }}>
              {STEP_LABELS.map((label, i) => (
                <button key={i} onClick={() => setStep(i)} style={{ padding: `${g(1)}px ${g(1.5)}px`, borderRadius: g(1.5), background: step === i ? ct.surface3 : "transparent", border: "none", fontSize: 10, fontWeight: 500, cursor: "pointer", color: step === i ? ct.labelPrimary : ct.labelTertiary }}>{label}</button>
              ))}
            </div>
          </div>
        </div>
      </div>

      {/* Progress dots */}
      <div style={{ display: "flex", justifyContent: "center", gap: g(1.5), padding: `${g(4)}px 0`, flexShrink: 0 }}>
        {steps.map((_, i) => (
          <div key={i} style={{
            width: step === i ? g(4) : g(1.5), height: g(1.5), borderRadius: g(1),
            background: i < step ? orbClr(flavor, t) : step === i ? orbClr(flavor, t) : t.surface3,
            opacity: i < step ? 0.4 : 1,
            transition: "all 200ms ease",
          }} />
        ))}
      </div>

      <StepComponent t={t} flavor={flavor} setFlavor={setFlavor} theme={theme} setTheme={setTheme} onNext={next} onBack={back} />

      <div style={{ height: g(8.5), display: "flex", justifyContent: "center", alignItems: "center", flexShrink: 0 }}>
        <div style={{ width: 134, height: 5, borderRadius: 3, background: t.surface3 }} />
      </div>
    </div>
  );
}