#!/usr/bin/env python3
"""Extract VoiceAccumulator class and wire into ChatActivity (#637 review items)"""
import sys

# --- ChatActivity.kt: wire VoiceAccumulator into beginListening ---
chat_path = '/Users/clawdiobot/citros/android/chat/src/main/kotlin/ai/citros/chat/ChatActivity.kt'

with open(chat_path, 'r') as f:
    chat = f.read()

# Add import for VoiceAccumulator
old_import = 'import ai.citros.core.SpeechEvent\n'
new_import = 'import ai.citros.core.SpeechEvent\nimport ai.citros.core.VoiceAccumulator\n'
if 'import ai.citros.core.VoiceAccumulator' not in chat:
    if old_import not in chat:
        print(f'ERROR: SpeechEvent import not found'); sys.exit(1)
    chat = chat.replace(old_import, new_import)
    print('Added VoiceAccumulator import')

# Replace beginListening to use VoiceAccumulator
old_fn = (
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

new_fn = (
    '    fun beginListening(stt: SpeechToTextProvider) {\n'
    '        // Cancel any previous listening session to avoid two AudioRecord\n'
    '        // instances fighting over the microphone (#637).\n'
    '        val previousJob = listeningJob\n'
    '        stt.stopListening()\n'
    '        val accumulator = VoiceAccumulator(prefix = text)\n'
    '        isListening = true\n'
    '        listeningJob = coroutineScope.launch {\n'
    '            // Wait for old AudioRecord cleanup to complete before creating\n'
    '            // a new one. cancel() is async \u2014 the old job\'s finally block\n'
    '            // (which calls audioRecord.stop()/release()) may not have run yet.\n'
    '            previousJob?.cancelAndJoin()\n'
    '            stt.startListening().collect { event ->\n'
    '                val display = accumulator.onEvent(event)\n'
    '                if (display != null) {\n'
    '                    text = display\n'
    '                }\n'
    '                if (event is SpeechEvent.Error) {\n'
    '                    isListening = false\n'
    '                    val err = event.error\n'
    '                    val errorMsg = when (err) {\n'
    '                        is SpeechError.PermissionDenied -> err.message\n'
    '                        is SpeechError.Unavailable -> err.message\n'
    '                        is SpeechError.Timeout -> err.message\n'
    '                        is SpeechError.EngineError -> err.message\n'
    '                        is SpeechError.NetworkError -> err.message\n'
    '                    }\n'
    '                    Toast.makeText(context, errorMsg, Toast.LENGTH_SHORT).show()\n'
    '                }\n'
    '            }\n'
    '            // Flow completed naturally (timeout or provider stopped).\n'
    '            // Auto-send the complete accumulated transcription if enabled.\n'
    '            val autoSend = voiceManager?.autoSendAfterVoice?.value == true\n'
    '            val result = accumulator.finish(autoSend = autoSend)\n'
    '            isListening = false\n'
    '            text = result.displayText\n'
    '            if (result.autoSendText != null) {\n'
    '                if (isLoading) onSteer(result.autoSendText) else onSend(result.autoSendText)\n'
    '            }\n'
    '        }\n'
    '    }')

if old_fn not in chat:
    print(f'ERROR: beginListening not found')
    idx = chat.find('fun beginListening')
    if idx >= 0:
        print(f'Found at index {idx}:')
        print(repr(chat[idx:idx+200]))
    sys.exit(1)

chat = chat.replace(old_fn, new_fn)
print('Updated beginListening to use VoiceAccumulator')

with open(chat_path, 'w') as f:
    f.write(chat)
print(f'OK: {chat_path}')

print('\nDone!')
