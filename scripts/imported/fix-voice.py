#!/usr/bin/env python3
"""Fix voice: multi-segment accumulation + text preservation (#637)

Two bugs:
1. Recording stops after first VAD segment (even a brief pause kills session)
2. Starting voice overwrites existing text in the input field

Fix:
1. STT emits multiple Finals, doesn't auto-stop. Session ends on timeout/manual stop.
2. ChatActivity preserves existing text and accumulates voice segments.
"""
import sys

# --- File 1: SherpaOnnxSpeechToText.kt ---
stt_path = '/Users/clawdiobot/citros/android/core/src/main/kotlin/ai/citros/core/SherpaOnnxSpeechToText.kt'

with open(stt_path, 'r') as f:
    stt = f.read()

changes = 0

# 1. Update KDoc: document multi-segment behavior
old = (' * The session ends after the first non-empty [SpeechEvent.Final] is emitted.\n'
       ' * Empty transcription results (e.g. from background noise that passes VAD) are\n'
       ' * silently discarded and the session continues listening.')
new = (' * Each non-empty transcription result is emitted as a [SpeechEvent.Final].\n'
       ' * The session continues listening until stopped via [stopListening], cancelled,\n'
       ' * or timed out. Callers should accumulate Finals to build the complete\n'
       ' * transcription. Empty transcription results (e.g. from background noise that\n'
       ' * passes VAD) are silently discarded and the session continues listening.')
if old not in stt:
    print(f'ERROR: KDoc not found in {stt_path}'); sys.exit(1)
stt = stt.replace(old, new); changes += 1

# 2. Remove gotFinal variable declaration
old2 = ('                    // Drain all completed speech segments from VAD queue\n'
        '                    var gotFinal = false\n')
new2 = '                    // Drain all completed speech segments from VAD queue\n'
if old2 not in stt:
    print(f'ERROR: gotFinal decl not found'); sys.exit(1)
stt = stt.replace(old2, new2); changes += 1

# 3. Replace emit block: remove break + gotFinal, add speechDetected reset
old3 = ('                        if (text.isNotEmpty()) {\n'
        '                            trySend(SpeechEvent.Final(text))\n'
        '                            gotFinal = true\n'
        '                            break // Stop after first successful transcription\n'
        '                        }\n'
        '                        // Empty results (noise, brief sounds) are silently discarded;\n'
        '                        // the session continues listening for real speech.\n'
        '                    }')
new3 = ('                        if (text.isNotEmpty()) {\n'
        '                            trySend(SpeechEvent.Final(text))\n'
        '                        }\n'
        '                        // Reset so next speech detection triggers a fresh Partial.\n'
        '                        // Empty results (noise, brief sounds) are silently discarded.\n'
        '                        speechDetected = false\n'
        '                    }')
if old3 not in stt:
    print(f'ERROR: emit block not found'); sys.exit(1)
stt = stt.replace(old3, new3); changes += 1

# 4. Remove gotFinal auto-stop block
old4 = ('\n                    if (gotFinal) {\n'
        '                        isListening.set(false)\n'
        '                    }\n')
if old4 not in stt:
    print(f'ERROR: gotFinal auto-stop not found'); sys.exit(1)
stt = stt.replace(old4, '\n'); changes += 1

with open(stt_path, 'w') as f:
    f.write(stt)
print(f'OK: {stt_path} ({changes} changes)')


# --- File 2: ChatActivity.kt ---
chat_path = '/Users/clawdiobot/citros/android/chat/src/main/kotlin/ai/citros/chat/ChatActivity.kt'

with open(chat_path, 'r') as f:
    chat = f.read()

old_fn = (
    '    fun beginListening(stt: SpeechToTextProvider) {\n'
    '        // Cancel any previous listening session to avoid two AudioRecord\n'
    '        // instances fighting over the microphone (#637).\n'
    '        val previousJob = listeningJob\n'
    '        stt.stopListening()\n'
    '        isListening = true\n'
    '        listeningJob = coroutineScope.launch {\n'
    '            // Wait for old AudioRecord cleanup to complete before creating\n'
    '            // a new one. cancel() is async \u2014 the old job\'s finally block\n'
    '            // (which calls audioRecord.stop()/release()) may not have run yet.\n'
    '            previousJob?.cancelAndJoin()\n'
    '            stt.startListening().collect { event ->\n'
    '                when (event) {\n'
    '                    is SpeechEvent.Partial -> text = event.text\n'
    '                    is SpeechEvent.Final -> {\n'
    '                        text = event.text\n'
    '                        isListening = false\n'
    '                        if (voiceManager?.autoSendAfterVoice?.value == true && event.text.isNotBlank()) {\n'
    '                            if (isLoading) onSteer(event.text) else onSend(event.text)\n'
    '                            text = ""\n'
    '                        }\n'
    '                    }\n'
    '                    is SpeechEvent.Error -> {\n'
    '                        isListening = false\n'
    '                        val err = event.error\n'
    '                        val errorMsg = when (err) {\n'
    '                            is SpeechError.PermissionDenied -> err.message\n'
    '                            is SpeechError.Unavailable -> err.message\n'
    '                            is SpeechError.Timeout -> err.message\n'
    '                            is SpeechError.EngineError -> err.message\n'
    '                            is SpeechError.NetworkError -> err.message\n'
    '                        }\n'
    '                        Toast.makeText(context, errorMsg, Toast.LENGTH_SHORT).show()\n'
    '                    }\n'
    '                }\n'
    '            }\n'
    '        }\n'
    '    }')

new_fn = (
    '    fun beginListening(stt: SpeechToTextProvider) {\n'
    '        // Cancel any previous listening session to avoid two AudioRecord\n'
    '        // instances fighting over the microphone (#637).\n'
    '        val previousJob = listeningJob\n'
    '        stt.stopListening()\n'
    '        val prefix = text  // Preserve existing text in the input field (#637)\n'
    '        isListening = true\n'
    '        listeningJob = coroutineScope.launch {\n'
    '            // Wait for old AudioRecord cleanup to complete before creating\n'
    '            // a new one. cancel() is async \u2014 the old job\'s finally block\n'
    '            // (which calls audioRecord.stop()/release()) may not have run yet.\n'
    '            previousJob?.cancelAndJoin()\n'
    '            var accumulated = ""\n'
    '            stt.startListening().collect { event ->\n'
    '                when (event) {\n'
    '                    is SpeechEvent.Partial -> {\n'
    '                        // Show existing text + accumulated voice + listening indicator\n'
    '                        val base = if (prefix.isNotBlank()) "$prefix " else ""\n'
    '                        text = if (accumulated.isEmpty()) {\n'
    '                            "${base}Listening..."\n'
    '                        } else {\n'
    '                            "$base$accumulated..."\n'
    '                        }\n'
    '                    }\n'
    '                    is SpeechEvent.Final -> {\n'
    '                        // Append segment to accumulated voice text (#637)\n'
    '                        accumulated = (accumulated + " " + event.text).trim()\n'
    '                        val base = if (prefix.isNotBlank()) "$prefix " else ""\n'
    '                        text = base + accumulated\n'
    '                    }\n'
    '                    is SpeechEvent.Error -> {\n'
    '                        isListening = false\n'
    '                        val err = event.error\n'
    '                        val errorMsg = when (err) {\n'
    '                            is SpeechError.PermissionDenied -> err.message\n'
    '                            is SpeechError.Unavailable -> err.message\n'
    '                            is SpeechError.Timeout -> err.message\n'
    '                            is SpeechError.EngineError -> err.message\n'
    '                            is SpeechError.NetworkError -> err.message\n'
    '                        }\n'
    '                        Toast.makeText(context, errorMsg, Toast.LENGTH_SHORT).show()\n'
    '                    }\n'
    '                }\n'
    '            }\n'
    '            // Flow completed naturally (timeout or provider stopped).\n'
    '            // Auto-send the complete accumulated transcription if enabled.\n'
    '            isListening = false\n'
    '            if (accumulated.isNotBlank() && voiceManager?.autoSendAfterVoice?.value == true) {\n'
    '                val finalText = (if (prefix.isNotBlank()) "$prefix " else "") + accumulated\n'
    '                if (isLoading) onSteer(finalText) else onSend(finalText)\n'
    '                text = ""\n'
    '            }\n'
    '        }\n'
    '    }')

if old_fn not in chat:
    print(f'ERROR: beginListening not found in {chat_path}')
    idx = chat.find('fun beginListening')
    if idx >= 0:
        print(f'Found at index {idx}:')
        print(repr(chat[idx:idx+300]))
    sys.exit(1)

chat = chat.replace(old_fn, new_fn)

with open(chat_path, 'w') as f:
    f.write(chat)
print(f'OK: {chat_path}')

print('\nAll edits applied successfully!')
