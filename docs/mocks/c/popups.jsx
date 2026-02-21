import { useState } from "react";

/*
 * DIRECTIVE C — Popups
 *
 * Two modal popups:
 *
 * 1. Add Provider — Bottom sheet triggered from Settings > API Keys > "Add provider".
 *    Segmented provider tabs, API key input (monospace, eye toggle),
 *    optional label, validate + save CTA. Follows the same pattern as the
 *    onboarding API Key step but adapted for a modal context.
 *
 * 2. Model Quick Switcher — Floating popover anchored below the nav bar
 *    subtitle ("Claude Sonnet 4.5"). Compact model list with checkmark,
 *    speed/capability badges, and a "Manage models" link to settings.
 *    Triggered by tapping the model name in the chat nav.
 *
 * Both popups use frosted overlay backgrounds (like Panel/Island overlays)
 * and are fully theme-aware.
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
    noFlavorSend: "#3A3A3C", noFlavorSendIcon: "rgba(235,235,245,0.60)",
  },
  light: {
    bg: "#FFFFFF", surface1: "#F2F2F7", surface2: "#E5E5EA", surface3: "#D1D1D6", surface4: "#C7C7CC",
    labelPrimary: "#000000", labelSecondary: "rgba(60,60,67,0.60)",
    labelTertiary: "rgba(60,60,67,0.30)", labelQuaternary: "rgba(60,60,67,0.18)",
    separator: "rgba(60,60,67,0.12)", separatorLight: "rgba(60,60,67,0.06)",
    green: "#34C759", red: "#FF3B30", orange: "#FF9500", blue: "#007AFF",
    noFlavorOrb: "#000000", noFlavorOrbInner: "rgba(255,255,255,0.20)",
    noFlavorGlow: "rgba(0,0,0,0.04)",
    noFlavorSend: "#D1D1D6", noFlavorSendIcon: "rgba(60,60,67,0.60)",
  },
};

const FLAVORS = {
  none:         { primary: null, onPrimary: null, glow: null, label: "None" },
  lemon:        { primary: "#FFD600", onPrimary: "#1C1A00", glow: "rgba(255,214,0,0.15)", label: "Lemon" },
  tangerine:    { primary: "#FF8C00", onPrimary: "#FFFFFF", glow: "rgba(255,140,0,0.15)", label: "Tangerine" },
  lime:         { primary: "#7CB342", onPrimary: "#FFFFFF", glow: "rgba(124,179,66,0.15)", label: "Lime" },
  blood_orange: { primary: "#D84315", onPrimary: "#FFFFFF", glow: "rgba(216,67,21,0.15)", label: "Blood Orange" },
  grapefruit:   { primary: "#E91E63", onPrimary: "#FFFFFF", glow: "rgba(233,30,99,0.15)", label: "Grapefruit" },
};

const orbClr = (fk, t) => fk === "none" ? t.noFlavorOrb : FLAVORS[fk].primary;
const orbInn = (fk, t) => fk === "none" ? t.noFlavorOrbInner : "rgba(0,0,0,0.12)";
const orbGlw = (fk, t) => fk === "none" ? t.noFlavorGlow : FLAVORS[fk].glow;
const accent = (fk, t) => fk === "none" ? t.labelSecondary : FLAVORS[fk].primary;
const btnBg = (fk, t) => fk === "none" ? t.labelPrimary : FLAVORS[fk].primary;
const btnText = (fk, t) => fk === "none" ? t.bg : (FLAVORS[fk].onPrimary || t.bg);

/* ── Icons ── */
const EyeIcon = (c) => <svg width="20" height="20" viewBox="0 0 20 20" fill="none"><ellipse cx="10" cy="10" rx="7.5" ry="4.5" stroke={c} strokeWidth="1.5"/><circle cx="10" cy="10" r="2" stroke={c} strokeWidth="1.5"/></svg>;
const EyeOffIcon = (c) => <svg width="20" height="20" viewBox="0 0 20 20" fill="none"><ellipse cx="10" cy="10" rx="7.5" ry="4.5" stroke={c} strokeWidth="1.5"/><circle cx="10" cy="10" r="2" stroke={c} strokeWidth="1.5"/><line x1="4" y1="16" x2="16" y2="4" stroke={c} strokeWidth="1.5" strokeLinecap="round"/></svg>;
const CheckIcon = (c) => <svg width="14" height="14" viewBox="0 0 14 14" fill="none"><path d="M3 7.5L6 10.5 11 4" stroke={c} strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"/></svg>;
const CloseIcon = (c) => <svg width="14" height="14" viewBox="0 0 14 14" fill="none"><path d="M3 3l8 8M11 3l-8 8" stroke={c} strokeWidth="1.5" strokeLinecap="round"/></svg>;
const ChevronIcon = (c) => <svg width="7" height="12" viewBox="0 0 7 12" fill="none"><path d="M1 1l5 5-5 5" stroke={c} strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/></svg>;
const GearIcon = (c) => <svg width="16" height="16" viewBox="0 0 16 16" fill="none"><circle cx="8" cy="8" r="2" stroke={c} strokeWidth="1.2"/><path d="M8 2v1.2M8 12.8V14M2 8h1.2M12.8 8H14M3.5 3.5l.85.85M11.65 11.65l.85.85M3.5 12.5l.85-.85M11.65 4.35l.85-.85" stroke={c} strokeWidth="1.2" strokeLinecap="round"/></svg>;

/* ── Orb ── */
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

/* ── Badge ── */
const Badge = ({ text, color }) => (
  <span style={{
    fontSize: 11, fontWeight: 500, letterSpacing: -0.1,
    padding: `${g(0.5)}px ${g(1.5)}px`, borderRadius: g(1),
    background: `${color}18`, color,
  }}>{text}</span>
);

/* ══════════════════════════════════════════════════════════════
   PROVIDERS CONFIGURATION
   ══════════════════════════════════════════════════════════════ */
const PROVIDERS = [
  { id: "anthropic",  label: "Anthropic",  url: "console.anthropic.com/settings/keys", prefix: "sk-ant-" },
  { id: "openrouter", label: "OpenRouter", url: "openrouter.ai/keys",                  prefix: "sk-or-" },
  { id: "openai",     label: "OpenAI",     url: "platform.openai.com/api-keys",        prefix: "sk-" },
];

/* ══════════════════════════════════════════════════════════════
   POPUP 1: Add Provider (Bottom Sheet)
   ══════════════════════════════════════════════════════════════ */
const AddProviderPopup = ({ t, flavor, state = "empty" }) => {
  const isDark = t.bg === "#000000";
  const ac = accent(flavor, t);

  /* State presets for the mock */
  const presets = {
    empty:     { provider: 0, apiKey: "",                          label: "",             showKey: false, validated: false, validating: false, error: null },
    typing:    { provider: 0, apiKey: "sk-ant-api03-xK9mP",       label: "",             showKey: true,  validated: false, validating: false, error: null },
    masked:    { provider: 0, apiKey: "sk-ant-api03-xK9mP2qR",    label: "Personal",     showKey: false, validated: false, validating: false, error: null },
    validating:{ provider: 0, apiKey: "sk-ant-api03-xK9mP2qR7Lw", label: "Personal",     showKey: false, validated: false, validating: true,  error: null },
    valid:     { provider: 0, apiKey: "sk-ant-api03-xK9mP2qR7Lw", label: "Personal key", showKey: false, validated: true,  validating: false, error: null },
    error:     { provider: 2, apiKey: "sk-invalid-key-123",        label: "",             showKey: false, validated: false, validating: false, error: "Invalid API key. Check it and try again." },
  };
  const s = presets[state] || presets.empty;
  const prov = PROVIDERS[s.provider];
  const hasKey = s.apiKey.length > 0;

  return (
    <div style={{
      position: "absolute", bottom: 0, left: 0, right: 0,
      borderRadius: `${g(5)}px ${g(5)}px 0 0`,
      background: isDark ? "rgba(28,28,30,0.96)" : "rgba(242,242,247,0.96)",
      backdropFilter: "blur(40px)",
      display: "flex", flexDirection: "column",
      overflow: "hidden",
      boxShadow: isDark ? "0 -4px 32px rgba(0,0,0,0.5)" : "0 -4px 32px rgba(0,0,0,0.12)",
    }}>
      {/* Grab handle */}
      <div style={{ padding: `${g(2.5)}px 0 ${g(1)}px`, display: "flex", justifyContent: "center" }}>
        <div style={{ width: g(9), height: 5, borderRadius: 3, background: t.surface3 }} />
      </div>

      {/* Header */}
      <div style={{
        display: "flex", alignItems: "center",
        padding: `${g(1)}px ${g(5)}px ${g(3)}px`,
      }}>
        <div style={{ flex: 1 }}>
          <div style={{ fontSize: 20, fontWeight: 700, letterSpacing: -0.4, color: t.labelPrimary }}>Add Provider</div>
        </div>
        <button style={{
          width: g(7), height: g(7), borderRadius: g(3.5),
          background: t.surface3, border: "none", cursor: "pointer",
          display: "grid", placeItems: "center",
        }}>
          {CloseIcon(t.labelSecondary)}
        </button>
      </div>

      {/* Segmented provider tabs */}
      <div style={{
        display: "flex", borderRadius: g(2.5),
        background: isDark ? t.surface1 : t.surface2,
        padding: g(0.75), margin: `0 ${g(5)}px ${g(4)}px`,
      }}>
        {PROVIDERS.map((p, i) => {
          const active = s.provider === i;
          return (
            <button key={p.id} style={{
              flex: 1, padding: `${g(2)}px 0`, borderRadius: g(2),
              background: active ? (isDark ? t.surface2 : t.bg) : "transparent",
              border: active ? `1px solid ${t.separator}` : "1px solid transparent",
              fontSize: 14, fontWeight: active ? 600 : 400,
              letterSpacing: -0.15, cursor: "pointer",
              color: active ? t.labelPrimary : t.labelTertiary,
              boxShadow: active ? "0 1px 3px rgba(0,0,0,0.12)" : "none",
              fontFamily: "inherit",
            }}>
              {p.label}
            </button>
          );
        })}
      </div>

      {/* Provider key link */}
      <div style={{ padding: `0 ${g(5)}px`, marginBottom: g(4) }}>
        <span style={{ fontSize: 13, color: ac, letterSpacing: -0.1 }}>
          Get a key from {prov.url}
        </span>
      </div>

      {/* API Key input */}
      <div style={{ padding: `0 ${g(5)}px`, marginBottom: g(3) }}>
        <div style={{ fontSize: 13, fontWeight: 400, color: t.labelSecondary, textTransform: "uppercase", letterSpacing: 0.5, marginBottom: g(2) }}>API Key</div>
        <div style={{
          display: "flex", alignItems: "center",
          background: isDark ? t.surface1 : t.bg,
          borderRadius: g(3), padding: `0 ${g(3)}px`, height: g(12),
          border: s.error ? `1.5px solid ${t.red}` : `1.5px solid transparent`,
          transition: "border-color 0.15s",
        }}>
          <div style={{
            flex: 1, fontSize: 16, letterSpacing: s.showKey ? -0.2 : 0.5,
            color: hasKey ? t.labelPrimary : t.labelTertiary,
            fontFamily: "SF Mono, 'Fira Code', monospace",
            overflow: "hidden", whiteSpace: "nowrap", textOverflow: "ellipsis",
          }}>
            {hasKey
              ? (s.showKey ? s.apiKey : `${s.apiKey.slice(0, 8)}${"•".repeat(Math.max(0, s.apiKey.length - 8))}`)
              : `${prov.prefix}...`
            }
          </div>
          <button style={{
            background: "none", border: "none", cursor: "pointer",
            padding: g(1), display: "grid", placeItems: "center", flexShrink: 0,
          }}>
            {s.showKey ? EyeOffIcon(t.labelTertiary) : EyeIcon(t.labelTertiary)}
          </button>
        </div>
        {/* Error message */}
        {s.error && (
          <div style={{ fontSize: 13, color: t.red, marginTop: g(1.5), letterSpacing: -0.1 }}>
            {s.error}
          </div>
        )}
      </div>

      {/* Label input */}
      <div style={{ padding: `0 ${g(5)}px`, marginBottom: g(5) }}>
        <div style={{ fontSize: 13, fontWeight: 400, color: t.labelSecondary, textTransform: "uppercase", letterSpacing: 0.5, marginBottom: g(2) }}>
          Label <span style={{ textTransform: "none", fontWeight: 400, color: t.labelTertiary }}>(optional)</span>
        </div>
        <div style={{
          display: "flex", alignItems: "center",
          background: isDark ? t.surface1 : t.bg,
          borderRadius: g(3), padding: `0 ${g(3)}px`, height: g(12),
        }}>
          <div style={{
            flex: 1, fontSize: 16, letterSpacing: -0.2,
            color: s.label ? t.labelPrimary : t.labelTertiary,
          }}>
            {s.label || "e.g. Personal key"}
          </div>
        </div>
      </div>

      {/* CTA Buttons */}
      <div style={{ padding: `0 ${g(5)}px ${g(8)}px` }}>
        {/* Validate / Save */}
        {s.validated ? (
          /* Validated state: green badge + Save button */
          <>
            <div style={{
              display: "flex", alignItems: "center", justifyContent: "center",
              gap: g(2), marginBottom: g(3),
              padding: `${g(2)}px 0`,
            }}>
              <div style={{
                width: g(5), height: g(5), borderRadius: g(2.5),
                background: `${t.green}20`, display: "grid", placeItems: "center",
              }}>
                <svg width="10" height="10" viewBox="0 0 10 10" fill="none"><path d="M2 5.5L4.2 7.5 8 3" stroke={t.green} strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round"/></svg>
              </div>
              <span style={{ fontSize: 14, fontWeight: 500, color: t.green, letterSpacing: -0.15 }}>Key verified</span>
            </div>
            <button style={{
              width: "100%", padding: `${g(3.5)}px 0`, borderRadius: g(3),
              background: btnBg(flavor, t), border: "none",
              fontSize: 17, fontWeight: 600, letterSpacing: -0.2,
              color: btnText(flavor, t), cursor: "pointer",
              fontFamily: "inherit",
            }}>
              Save Provider
            </button>
          </>
        ) : (
          /* Not yet validated: Validate Key button */
          <button style={{
            width: "100%", padding: `${g(3.5)}px 0`, borderRadius: g(3),
            background: hasKey && !s.validating ? btnBg(flavor, t) : (isDark ? t.surface2 : t.surface3),
            border: "none",
            fontSize: 17, fontWeight: 600, letterSpacing: -0.2,
            color: hasKey && !s.validating ? btnText(flavor, t) : t.labelTertiary,
            cursor: hasKey ? "pointer" : "default",
            fontFamily: "inherit",
            opacity: s.validating ? 0.7 : 1,
          }}>
            {s.validating ? "Validating..." : "Validate Key"}
          </button>
        )}
      </div>
    </div>
  );
};

/* ══════════════════════════════════════════════════════════════
   POPUP 2: Model Quick Switcher (Floating Popover)
   ══════════════════════════════════════════════════════════════ */
const MODELS = [
  { id: "llama_local",    name: "llama.cpp",         detail: "On-device · Fastest",      badge: "Local",    badgeColor: "green",  tier: "fast" },
  { id: "claude_sonnet",  name: "Claude Sonnet 4.5", detail: "Cloud · Balanced",          badge: "Cloud",    badgeColor: "blue",   tier: "balanced" },
  { id: "claude_opus",    name: "Claude Opus 4.5",   detail: "Cloud · Most capable",      badge: "Cloud",    badgeColor: "blue",   tier: "capable" },
  { id: "gpt4o",          name: "GPT-4o",            detail: "Cloud · Fast multi-modal",  badge: "Cloud",    badgeColor: "blue",   tier: "balanced" },
];

const ModelSwitcherPopup = ({ t, flavor, selected = 1 }) => {
  const isDark = t.bg === "#000000";
  const ac = accent(flavor, t);

  return (
    <div style={{
      position: "absolute", top: g(15), left: g(4), right: g(4),
      borderRadius: g(4),
      background: isDark ? "rgba(28,28,30,0.96)" : "rgba(255,255,255,0.96)",
      backdropFilter: "blur(40px)",
      boxShadow: isDark
        ? "0 8px 32px rgba(0,0,0,0.5), 0 1px 4px rgba(0,0,0,0.3)"
        : "0 8px 32px rgba(0,0,0,0.12), 0 1px 4px rgba(0,0,0,0.06)",
      border: isDark ? "none" : `1px solid ${t.separator}`,
      overflow: "hidden",
    }}>
      {/* Header */}
      <div style={{
        padding: `${g(3.5)}px ${g(4)}px ${g(2)}px`,
        display: "flex", alignItems: "center",
      }}>
        <span style={{ fontSize: 13, fontWeight: 400, color: t.labelSecondary, textTransform: "uppercase", letterSpacing: 0.5 }}>Model</span>
        <div style={{ flex: 1 }} />
        <button style={{
          display: "flex", alignItems: "center", gap: g(1),
          background: "none", border: "none", cursor: "pointer",
          padding: `${g(1)}px 0`,
        }}>
          {GearIcon(t.labelTertiary)}
          <span style={{ fontSize: 13, color: t.labelTertiary, letterSpacing: -0.1 }}>Manage</span>
        </button>
      </div>

      {/* Model list */}
      <div style={{ padding: `0 ${g(2)}px ${g(2)}px` }}>
        {MODELS.map((m, i) => {
          const isSelected = selected === i;
          return (
            <div key={m.id} style={{
              display: "flex", alignItems: "center", gap: g(3),
              padding: `${g(2.5)}px ${g(2.5)}px`,
              borderRadius: g(2.5),
              background: isSelected ? (isDark ? t.surface2 : t.surface1) : "transparent",
              cursor: "pointer",
              transition: "background 0.15s",
              marginBottom: i < MODELS.length - 1 ? 1 : 0,
            }}>
              {/* Model icon / tier indicator */}
              <div style={{
                width: g(9), height: g(9), borderRadius: g(2.5),
                background: isSelected
                  ? (isDark ? t.surface3 : t.surface2)
                  : (isDark ? t.surface1 : t.surface1),
                display: "grid", placeItems: "center", flexShrink: 0,
              }}>
                {m.tier === "fast" ? (
                  /* Lightning bolt for local/fast */
                  <svg width="14" height="14" viewBox="0 0 14 14" fill="none"><path d="M8 1L3 8h4l-1 5 5-7H7l1-5z" stroke={isSelected ? ac : t.labelTertiary} strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round"/></svg>
                ) : m.tier === "capable" ? (
                  /* Star for most capable */
                  <svg width="14" height="14" viewBox="0 0 14 14" fill="none"><path d="M7 2l1.5 3.1 3.4.5-2.5 2.4.6 3.4L7 9.8 4 11.4l.6-3.4-2.5-2.4 3.4-.5L7 2z" stroke={isSelected ? ac : t.labelTertiary} strokeWidth="1.2" strokeLinejoin="round"/></svg>
                ) : (
                  /* Balanced: circle with dot */
                  <svg width="14" height="14" viewBox="0 0 14 14" fill="none"><circle cx="7" cy="7" r="4.5" stroke={isSelected ? ac : t.labelTertiary} strokeWidth="1.2"/><circle cx="7" cy="7" r="1.5" fill={isSelected ? ac : t.labelTertiary}/></svg>
                )}
              </div>

              {/* Name + detail */}
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{
                  fontSize: 15, fontWeight: isSelected ? 600 : 400,
                  letterSpacing: -0.2, color: t.labelPrimary,
                  lineHeight: "20px",
                }}>{m.name}</div>
                <div style={{
                  fontSize: 12, letterSpacing: -0.1,
                  color: t.labelTertiary, lineHeight: "16px",
                  marginTop: 1,
                }}>{m.detail}</div>
              </div>

              {/* Badge */}
              <Badge text={m.badge} color={t[m.badgeColor]} />

              {/* Checkmark */}
              {isSelected && (
                <div style={{ flexShrink: 0, marginLeft: g(0.5) }}>
                  {CheckIcon(ac)}
                </div>
              )}
            </div>
          );
        })}
      </div>

      {/* Fallback toggle */}
      <div style={{
        borderTop: `0.5px solid ${t.separator}`,
        padding: `${g(2.5)}px ${g(4)}px ${g(3)}px`,
        display: "flex", alignItems: "center",
      }}>
        <div style={{ flex: 1 }}>
          <div style={{ fontSize: 14, letterSpacing: -0.15, color: t.labelPrimary }}>Use local when offline</div>
        </div>
        <div style={{
          width: 42, height: 26, borderRadius: 13,
          background: t.green, padding: 2, cursor: "pointer",
          display: "flex", alignItems: "center",
        }}>
          <div style={{
            width: 22, height: 22, borderRadius: 11,
            background: "#fff", transform: "translateX(16px)",
            boxShadow: "0 1px 3px rgba(0,0,0,0.3)",
          }} />
        </div>
      </div>
    </div>
  );
};

/* ══════════════════════════════════════════════════════════════
   PHONE FRAME
   ══════════════════════════════════════════════════════════════ */
const PhoneFrame = ({ children, t, dimmed = false }) => {
  const isDark = t.bg === "#000000";
  return (
    <div style={{
      width: 390, height: 844, borderRadius: g(10), overflow: "hidden",
      position: "relative", background: t.bg,
      border: `1px solid ${t.separator}`, flexShrink: 0,
      fontFamily: "-apple-system, 'SF Pro Text', system-ui, sans-serif",
      WebkitFontSmoothing: "antialiased",
    }}>
      {/* Status bar */}
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", padding: `${g(3)}px ${g(7)}px`, opacity: 0.3 }}>
        <span style={{ fontSize: 14, fontWeight: 600, color: t.labelPrimary }}>12:30</span>
        <div style={{ width: 16, height: 10, borderRadius: 2, border: `1.5px solid ${t.labelPrimary}` }} />
      </div>

      {/* Backdrop dim */}
      {dimmed && (
        <div style={{
          position: "absolute", inset: 0, zIndex: 5,
          background: isDark ? "rgba(0,0,0,0.3)" : "rgba(0,0,0,0.15)",
        }} />
      )}

      {/* Content */}
      <div style={{ position: "relative", zIndex: dimmed ? 10 : 1, height: "calc(100% - 56px)" }}>
        {children}
      </div>
    </div>
  );
};

/* ══════════════════════════════════════════════════════════════
   MOCK CONTEXT: Settings background (for Add Provider)
   ══════════════════════════════════════════════════════════════ */
const SettingsBackground = ({ t, flavor }) => (
  <div style={{ padding: `0 ${g(4)}px`, opacity: 0.4, filter: "blur(1px)" }}>
    <div style={{ fontSize: 17, fontWeight: 600, letterSpacing: -0.3, color: t.labelPrimary, textAlign: "center", marginBottom: g(4) }}>API Keys</div>
    <div style={{ background: t.surface1, borderRadius: g(3), overflow: "hidden" }}>
      <div style={{ display: "flex", alignItems: "center", padding: `${g(3)}px ${g(4)}px`, borderBottom: `0.5px solid ${t.separatorLight}` }}>
        <div style={{ flex: 1 }}>
          <div style={{ fontSize: 16, color: t.labelPrimary, letterSpacing: -0.2 }}>Anthropic</div>
          <div style={{ fontSize: 13, color: t.labelTertiary, marginTop: 1 }}>sk-ant-•••••4f2a</div>
        </div>
        <Badge text="Active" color={t.green} />
      </div>
      <div style={{ display: "flex", alignItems: "center", padding: `${g(3)}px ${g(4)}px` }}>
        <div style={{ flex: 1 }}>
          <div style={{ fontSize: 16, color: t.labelPrimary, letterSpacing: -0.2 }}>OpenAI</div>
          <div style={{ fontSize: 13, color: t.labelTertiary, marginTop: 1 }}>sk-•••••8b7c</div>
        </div>
        <Badge text="Active" color={t.green} />
      </div>
    </div>
  </div>
);

/* ══════════════════════════════════════════════════════════════
   MOCK CONTEXT: Chat background (for Model Switcher)
   ══════════════════════════════════════════════════════════════ */
const ChatNavAndBackground = ({ t, flavor }) => (
  <div>
    {/* Nav bar */}
    <nav style={{
      display: "flex", alignItems: "center",
      padding: `${g(3)}px ${g(4)}px`, gap: g(3),
      borderBottom: `0.5px solid ${t.separator}`, height: g(11),
    }}>
      <Orb color={orbClr(flavor, t)} inner={orbInn(flavor, t)} glow={orbGlw(flavor, t)} size={g(8)} />
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontSize: 17, fontWeight: 600, letterSpacing: -0.4, color: t.labelPrimary, lineHeight: "22px" }}>Citros</div>
        <div style={{ fontSize: 13, letterSpacing: -0.1, color: accent(flavor, t), lineHeight: "18px", textDecoration: "underline", textDecorationStyle: "dotted", textUnderlineOffset: 2 }}>
          Claude Sonnet 4.5 ▾
        </div>
      </div>
      <button style={{ width: g(11), height: g(11), display: "grid", placeItems: "center", background: "none", border: "none", cursor: "pointer", borderRadius: g(3) }}>
        {GearIcon(t.labelSecondary)}
      </button>
    </nav>
    {/* Blurred chat preview */}
    <div style={{ padding: `${g(4)}px ${g(4)}px`, opacity: 0.3, filter: "blur(1px)" }}>
      <div style={{ alignSelf: "flex-start", maxWidth: "75%", padding: `${g(2.5)}px ${g(3.5)}px`, borderRadius: "18px 18px 18px 4px", background: t.surface2, fontSize: 14, color: t.labelPrimary, lineHeight: "20px", marginBottom: g(2) }}>
        Done. You'll get a notification 10 minutes before.
      </div>
      <div style={{ alignSelf: "flex-end", maxWidth: "70%", padding: `${g(2.5)}px ${g(3.5)}px`, borderRadius: "18px 18px 4px 18px", background: orbClr(flavor, t), fontSize: 14, color: FLAVORS[flavor]?.onPrimary || t.labelPrimary, lineHeight: "20px", marginLeft: "auto", marginBottom: g(2) }}>
        Keep it short. Three bullet points max.
      </div>
      <div style={{ alignSelf: "flex-start", maxWidth: "80%", padding: `${g(2.5)}px ${g(3.5)}px`, borderRadius: "18px 18px 18px 4px", background: t.surface2, fontSize: 14, color: t.labelPrimary, lineHeight: "20px" }}>
        Here's a draft agenda for your meeting with Sarah...
      </div>
    </div>
  </div>
);

/* ══════════════════════════════════════════════════════════════
   MAIN: Renders all popup combinations
   ══════════════════════════════════════════════════════════════ */
export default function PopupsScreen() {
  const [flavor, setFlavor] = useState("tangerine");
  const [theme, setTheme] = useState("dark");
  const [popup, setPopup] = useState("provider");
  const [providerState, setProviderState] = useState("empty");
  const [modelSelected, setModelSelected] = useState(1);
  const t = themes[theme];
  const ct = themes.dark;

  const providerStates = ["empty", "typing", "masked", "validating", "valid", "error"];

  return (
    <div style={{
      fontFamily: "-apple-system, 'SF Pro Text', system-ui, sans-serif",
      background: "#111", minHeight: "100vh",
      display: "flex", flexDirection: "column", alignItems: "center",
      padding: `${g(8)}px ${g(4)}px`,
      WebkitFontSmoothing: "antialiased", color: ct.labelPrimary,
    }}>
      {/* Controls */}
      <div style={{ width: "100%", maxWidth: 460, marginBottom: g(5) }}>
        <h1 style={{ fontSize: 24, fontWeight: 700, letterSpacing: -0.6, margin: 0 }}>Popup Modals</h1>
        <p style={{ fontSize: 15, color: ct.labelSecondary, letterSpacing: -0.2, margin: `${g(1.5)}px 0 ${g(5)}px`, lineHeight: "22px" }}>
          Add Provider bottom sheet & Model Quick Switcher popover.
        </p>

        <div style={{ display: "flex", gap: g(4), flexWrap: "wrap", marginBottom: g(5) }}>
          <div>
            <div style={{ fontSize: 11, fontWeight: 600, color: ct.labelTertiary, letterSpacing: 0.3, textTransform: "uppercase", marginBottom: g(2) }}>Popup</div>
            <div style={{ display: "flex", gap: g(1.5) }}>
              {["provider", "switcher"].map(p => (
                <button key={p} onClick={() => setPopup(p)} style={{ padding: `${g(1.5)}px ${g(3)}px`, borderRadius: g(2), background: popup === p ? ct.surface3 : "transparent", border: "none", fontSize: 12, fontWeight: 500, cursor: "pointer", color: popup === p ? ct.labelPrimary : ct.labelTertiary, textTransform: "capitalize" }}>
                  {p === "provider" ? "Add Provider" : "Model Switcher"}
                </button>
              ))}
            </div>
          </div>

          {popup === "provider" && (
            <div>
              <div style={{ fontSize: 11, fontWeight: 600, color: ct.labelTertiary, letterSpacing: 0.3, textTransform: "uppercase", marginBottom: g(2) }}>State</div>
              <div style={{ display: "flex", gap: g(1.5), flexWrap: "wrap" }}>
                {providerStates.map(s => (
                  <button key={s} onClick={() => setProviderState(s)} style={{ padding: `${g(1.5)}px ${g(3)}px`, borderRadius: g(2), background: providerState === s ? ct.surface3 : "transparent", border: "none", fontSize: 12, fontWeight: 500, cursor: "pointer", color: providerState === s ? ct.labelPrimary : ct.labelTertiary, textTransform: "capitalize" }}>{s}</button>
                ))}
              </div>
            </div>
          )}

          {popup === "switcher" && (
            <div>
              <div style={{ fontSize: 11, fontWeight: 600, color: ct.labelTertiary, letterSpacing: 0.3, textTransform: "uppercase", marginBottom: g(2) }}>Selected</div>
              <div style={{ display: "flex", gap: g(1.5), flexWrap: "wrap" }}>
                {MODELS.map((m, i) => (
                  <button key={m.id} onClick={() => setModelSelected(i)} style={{ padding: `${g(1.5)}px ${g(3)}px`, borderRadius: g(2), background: modelSelected === i ? ct.surface3 : "transparent", border: "none", fontSize: 12, fontWeight: 500, cursor: "pointer", color: modelSelected === i ? ct.labelPrimary : ct.labelTertiary }}>{m.name.split(" ")[0]}</button>
                ))}
              </div>
            </div>
          )}

          <div>
            <div style={{ fontSize: 11, fontWeight: 600, color: ct.labelTertiary, letterSpacing: 0.3, textTransform: "uppercase", marginBottom: g(2) }}>Theme</div>
            <div style={{ display: "flex", gap: g(1.5) }}>
              {["dark", "light"].map(th => (
                <button key={th} onClick={() => setTheme(th)} style={{ padding: `${g(1.5)}px ${g(3)}px`, borderRadius: g(2), background: theme === th ? ct.surface3 : "transparent", border: "none", fontSize: 12, fontWeight: 500, cursor: "pointer", color: theme === th ? ct.labelPrimary : ct.labelTertiary, textTransform: "capitalize" }}>{th}</button>
              ))}
            </div>
          </div>

          <div>
            <div style={{ fontSize: 11, fontWeight: 600, color: ct.labelTertiary, letterSpacing: 0.3, textTransform: "uppercase", marginBottom: g(2) }}>Flavor</div>
            <div style={{ display: "flex", gap: g(2) }}>
              <button onClick={() => setFlavor("none")} style={{ width: g(6), height: g(6), borderRadius: g(3), cursor: "pointer", background: "linear-gradient(135deg, #fff 50%, #000 50%)", border: flavor === "none" ? `2px solid ${ct.labelPrimary}` : "2px solid transparent" }} />
              {Object.entries(FLAVORS).filter(([k]) => k !== "none").map(([n, fl]) => (
                <button key={n} onClick={() => setFlavor(n)} style={{ width: g(6), height: g(6), borderRadius: g(3), background: fl.primary, cursor: "pointer", border: flavor === n ? `2px solid ${ct.labelPrimary}` : "2px solid transparent" }} />
              ))}
            </div>
          </div>
        </div>
      </div>

      {/* Phone preview */}
      <PhoneFrame t={t} dimmed={true}>
        {popup === "provider" ? (
          <>
            <SettingsBackground t={t} flavor={flavor} />
            <AddProviderPopup t={t} flavor={flavor} state={providerState} />
          </>
        ) : (
          <>
            <ChatNavAndBackground t={t} flavor={flavor} />
            <ModelSwitcherPopup t={t} flavor={flavor} selected={modelSelected} />
          </>
        )}
      </PhoneFrame>
    </div>
  );
}
