# Privacy Policy

**Fawx AI LLC**
**Effective Date: March 1, 2026**

This Privacy Policy describes how Fawx AI LLC, a Colorado limited liability company ("Fawx AI," "we," "us," or "our") handles information when you use the Fawx application ("Software"). We believe in straightforward privacy: **Fawx collects nothing by default.**

---

## 1. Our Privacy Principle

Fawx runs entirely on your device. By default:

- **No telemetry** — disabled by default, opt-in only
- **No analytics** — no tracking pixels, no usage analytics, no behavioral profiling
- **No cloud storage** — all your data stays on your device
- **No account required** — Fawx works without creating an account with us
- **No API key transmission** — your keys are encrypted locally, never sent to us

If you never enable telemetry, we collect **zero data** from you. You don't need to trust us with your data — we designed the system so we never have it.

---

## 2. What We Collect (When You Opt In)

If you explicitly enable telemetry in **Settings → Privacy & Telemetry**, we collect the following behavioral signals. These are designed to help us improve Fawx without ever seeing your content.

### 2.1 Tool Usage

Which tools succeed or fail, and how often. We see patterns like "the file-write tool failed 3 times today" — **not** which files were written or what content was involved.

### 2.2 Proposal Gate Activity

How often the safety gate fires. We see aggregate counts like "the proposal gate activated 12 times" — **not** what actions were proposed or blocked.

### 2.3 Experiment Metrics

Scores and decisions from experiments (e.g., "experiment A scored 0.87 and was selected"). We see **no code content**, prompt text, or generated output.

### 2.4 Error Categories

Error types and their frequency. Error messages are **hashed before transmission** — we see error patterns, not raw error text that might contain sensitive information.

### 2.5 Model Usage

Which AI models and thinking levels you use (e.g., "Claude Opus, high thinking"). We see **no conversations**, prompts, or responses.

### 2.6 Performance Metrics

Response times and token counts. We see performance data like "average response: 2.3 seconds, 450 tokens" — **not** the prompts or responses themselves.

---

## 3. What We Never Collect

Regardless of your telemetry settings, we **never** collect:

- ❌ **Conversation content** — your prompts, AI responses, or chat history
- ❌ **File contents or paths** — what's on your device stays on your device
- ❌ **API keys, tokens, or credentials** — stored locally in an encrypted credential store, never transmitted
- ❌ **IP addresses** — telemetry session IDs are random and reset on every restart
- ❌ **Device identifiers** — no hardware fingerprints, serial numbers, or advertising IDs
- ❌ **Personal information** — no names, emails, phone numbers, or account details
- ❌ **Browsing history or app usage** outside of Fawx
- ❌ **Location data**

---

## 4. Third-Party Data Sharing

**We do not sell, share, or provide your data to any third party.** Period.

- No advertising partners
- No data brokers
- No analytics services
- No "anonymized" data sharing

If this ever changes in the future, it will require a **new, explicit opt-in consent flow** — not a policy update buried in an email.

---

## 5. Data Retention

Telemetry data is handled with minimal retention:

- Telemetry signals are **buffered in memory only** — they are not persisted to disk
- If you restart Fawx, the buffer is **automatically cleared**
- If you disable telemetry, all buffered signals are **immediately deleted**

### Future Upload Capability

If and when we add the ability to upload telemetry data to our servers, we will implement:

- **Differential privacy** — data is aggregated before storage; individual sessions cannot be reconstructed
- **90-day rolling retention** — data older than 90 days is automatically deleted
- **Data export** — you can request a copy of any data associated with your session (right to access)
- **Data deletion** — you can request deletion of any data associated with your session (right to erasure)

These capabilities will be described in an updated version of this policy before they go live.

---

## 6. Third-Party AI Providers

When you use Fawx to connect to AI providers like Anthropic or OpenAI:

- Your prompts and responses flow **directly between your device and the provider** — Fawx AI does not see, intercept, or store this data
- Your use of these providers is subject to **their privacy policies**:
  - [Anthropic Privacy Policy](https://www.anthropic.com/privacy)
  - [OpenAI Privacy Policy](https://openai.com/privacy)
- OAuth tokens for subscription-based access (e.g., ChatGPT) are stored in your **local encrypted credential store**, not on our servers
- We have **no access** to your provider accounts, usage history, or billing information

We encourage you to review each provider's privacy policy to understand how they handle your data.

---

## 7. Children's Privacy (COPPA Compliance)

Fawx is **not directed at children under 13 years of age.** We do not knowingly collect personal information from children under 13.

If you are a parent or guardian and believe your child under 13 has provided personal information to us, please contact us at the address below. We will promptly delete any such information.

Users between 13 and 18 may use Fawx with parental or guardian consent.

---

## 8. Security

While Fawx collects no data by default, we take the following measures to protect the Software and any optional telemetry:

- API keys are stored in a **platform-native encrypted credential store** (macOS Keychain, iOS Keychain)
- The kernel binary is **code-signed and notarized** by Apple
- All network communications use **TLS encryption**
- The safety architecture (proposal gate, kernel blindness) is designed to protect you from unintended agent actions

---

## 9. Your Rights

Depending on your jurisdiction, you may have the right to:

- **Access** any data we hold about you
- **Delete** any data we hold about you
- **Opt out** of telemetry at any time (Settings → Privacy & Telemetry)
- **Data portability** — receive your data in a portable format

Since Fawx collects no data by default, these rights are most relevant if you have opted into telemetry. To exercise any of these rights, contact us at the address below.

---

## 10. Changes to This Policy

We may update this Privacy Policy from time to time. When we make material changes:

- We will notify you **through the Fawx application**
- We will update the "Effective Date" at the top of this document
- For changes that expand data collection or sharing, we will require **new opt-in consent** — not just continued use

We will not retroactively apply less protective terms to data already collected.

---

## 11. Contact

If you have questions about this Privacy Policy or want to exercise your data rights, contact us at:

**Fawx AI LLC**
Email: privacy@fawx.ai

---

*This document is a template for initial product launch and does not constitute legal advice. Consult an attorney for review before relying on it.*
