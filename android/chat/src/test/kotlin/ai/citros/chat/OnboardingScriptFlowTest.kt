package ai.citros.chat

import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import kotlin.test.assertEquals
import kotlin.test.assertNotNull

/**
 * Unit tests for the new scripted variable-substitution onboarding flow.
 */
@RunWith(RobolectricTestRunner::class)
class OnboardingScriptFlowTest {

    @Test
    fun substituteVariables_withValidVariables_substitutesCorrectly() {
        val variables = mapOf(
            "agentName" to "Zest",
            "userName" to "Alice", 
            "style" to "casual"
        )
        
        val template = "Hi {userName}, I'm {agentName} and I'll keep things {style}."
        val result = substituteVariables(template, variables)
        
        assertEquals("Hi Alice, I'm Zest and I'll keep things casual.", result)
    }

    @Test
    fun substituteVariables_withMissingVariables_leavesPlaceholders() {
        val variables = mapOf(
            "agentName" to "Zest"
        )
        
        val template = "Hi {userName}, I'm {agentName}."
        val result = substituteVariables(template, variables)
        
        assertEquals("Hi {userName}, I'm Zest.", result)
    }

    @Test
    fun substituteVariables_withEmptyVariables_leavesAllPlaceholders() {
        val variables = emptyMap<String, String>()
        val template = "Hi {userName}, I'm {agentName}."
        val result = substituteVariables(template, variables)
        
        assertEquals("Hi {userName}, I'm {agentName}.", result)
    }

    @Test
    fun substituteVariables_withNoPlaceholders_returnsOriginal() {
        val variables = mapOf("agentName" to "Zest")
        val template = "Hello there! How can I help?"
        val result = substituteVariables(template, variables)
        
        assertEquals(template, result)
    }

    @Test
    fun buildProfileFromCapturedVariables_withAllVariables_buildsCorrectProfile() {
        val variables = mapOf(
            "agentName" to "Zest",
            "userName" to "Alice",
            "style" to "casual and friendly", 
            "boundaries" to "ask before sending messages"
        )
        
        val profile = buildProfileFromCapturedVariables(variables)
        
        assertEquals("Zest", profile.agentName)
        assertEquals("Alice", profile.userName)
        assertEquals("Alice", profile.userAddress)
        assertEquals("casual and friendly", profile.relationshipStyle)
        assertEquals("ask before sending messages", profile.boundaries)
        assertEquals("citrus spirit", profile.agentNature)
        assertEquals("🍋", profile.agentEmoji)
        assertEquals("chill but sharp", profile.agentVibe)
        assertEquals("onboarding completed", profile.userContext)
        assertEquals(1.0f, profile.confidence)
    }

    @Test
    fun buildProfileFromCapturedVariables_withMissingVariables_usesDefaults() {
        val variables = mapOf(
            "agentName" to "Bolt"
        )
        
        val profile = buildProfileFromCapturedVariables(variables)
        
        assertEquals("Bolt", profile.agentName)
        assertEquals("You", profile.userName) // default
        assertEquals("You", profile.userAddress) // default
        assertEquals("helpful and friendly", profile.relationshipStyle) // default
        assertEquals("ask before taking actions", profile.boundaries) // default
    }

    @Test
    fun buildProfileFromCapturedVariables_withEmptyVariables_usesAllDefaults() {
        val variables = emptyMap<String, String>()
        
        val profile = buildProfileFromCapturedVariables(variables)
        
        assertEquals("Citros", profile.agentName) // default
        assertEquals("You", profile.userName) // default
        assertEquals("helpful and friendly", profile.relationshipStyle) // default
        assertEquals("ask before taking actions", profile.boundaries) // default
    }

    @Test
    fun onboardingScriptSteps_hasCorrectStructure() {
        // Test that the script steps are defined correctly
        assertEquals(4, onboardingScriptSteps.size)
        
        // First step should ask for agent name
        val step1 = onboardingScriptSteps[0]
        assertEquals("agentName", step1.captureAs)
        assertNotNull(step1.question)
        assertNotNull(step1.responseTemplate)
        
        // Second step should ask for user name
        val step2 = onboardingScriptSteps[1]
        assertEquals("userName", step2.captureAs)
        assertNotNull(step2.question)
        assertNotNull(step2.responseTemplate)
        
        // Third step should ask for style
        val step3 = onboardingScriptSteps[2]
        assertEquals("style", step3.captureAs)
        assertNotNull(step3.question)
        assertNotNull(step3.responseTemplate)
        
        // Fourth step should capture boundaries
        val step4 = onboardingScriptSteps[3]
        assertEquals("boundaries", step4.captureAs)
        // Last step has null question as it's just capture
    }

    @Test
    fun onboardingScriptSteps_questionsDoNotUseUncapturedVariables() {
        // Verify that each step's question only uses variables captured in previous steps
        for (i in onboardingScriptSteps.indices) {
            val step = onboardingScriptSteps[i]
            val question = step.question ?: continue
            
            // Get variables captured in previous steps
            val availableVariables = onboardingScriptSteps.take(i).mapNotNull { it.captureAs }
            
            // Check that question doesn't reference variables not yet captured
            val allVariableNames = listOf("agentName", "userName", "style", "boundaries")
            for (varName in allVariableNames) {
                if (question.contains("{$varName}") && varName !in availableVariables) {
                    kotlin.test.fail("Step $i question uses uncaptured variable {$varName}: $question")
                }
            }
        }
    }
    
    @Test
    fun onboardingScriptSteps_responseTemplatesUseValidVariables() {
        // Test that response templates reference variables correctly
        val step1 = onboardingScriptSteps[0]
        val step2 = onboardingScriptSteps[1]
        val step3 = onboardingScriptSteps[2]
        val step4 = onboardingScriptSteps[3]
        
        // Step 1 response template should use {agentName} (captured in step 1)
        assertEquals(true, step1.responseTemplate?.contains("{agentName}"))
        
        // Step 2 response template should use {userName} (captured in step 2)
        assertEquals(true, step2.responseTemplate?.contains("{userName}"))
        
        // Step 3 response template should use {userName} (captured in step 2)
        assertEquals(true, step3.responseTemplate?.contains("{userName}"))
        
        // Step 4 response template should use multiple variables
        assertEquals(true, step4.responseTemplate?.contains("{agentName}"))
        assertEquals(true, step4.responseTemplate?.contains("{userName}"))
        assertEquals(true, step4.responseTemplate?.contains("{style}"))
    }
    
    @Test
    fun onboardingScriptSteps_responseTemplatesDoNotDuplicateNextQuestion() {
        // Regression test for #383: step N's responseTemplate should not contain
        // step N+1's question, which causes duplicate messages in the UI.
        for (i in 0 until onboardingScriptSteps.size - 1) {
            val currentResponse = onboardingScriptSteps[i].responseTemplate ?: continue
            val nextQuestion = onboardingScriptSteps[i + 1].question ?: continue
            assertEquals(
                false,
                currentResponse.contains(nextQuestion, ignoreCase = true),
                "Step $i response duplicates step ${i + 1} question: '$nextQuestion'"
            )
        }
    }

    @Test
    fun stepProgression_capturesVariablesInCorrectOrder() {
        // Test that variables are captured in the expected sequence
        val expectedCaptureOrder = listOf("agentName", "userName", "style", "boundaries")
        val actualCaptureOrder = onboardingScriptSteps.mapNotNull { it.captureAs }
        
        assertEquals(expectedCaptureOrder, actualCaptureOrder)
    }
    
    @Test 
    fun stepProgression_hasCorrectFlowStructure() {
        // Test the overall structure of the flow
        assertEquals(4, onboardingScriptSteps.size)
        
        // First 3 steps should have questions and response templates
        for (i in 0..2) {
            val step = onboardingScriptSteps[i]
            assertNotNull(step.question, "Step $i should have a question")
            assertNotNull(step.responseTemplate, "Step $i should have a response template")
            assertNotNull(step.captureAs, "Step $i should capture a variable")
        }
        
        // Last step should have question and captureAs but no response template
        val lastStep = onboardingScriptSteps[3]
        assertNotNull(lastStep.question, "Last step should have a question")
        assertNotNull(lastStep.captureAs, "Last step should capture a variable")
        assertNotNull(lastStep.responseTemplate, "Last step should have a response template")
    }
    
    @Test
    fun endToEndFlow_variableSubstitution() {
        // Test variable substitution works correctly through the entire flow
        val capturedVars = mutableMapOf<String, String>()
        
        // Simulate the flow step by step
        
        // Step 1: User provides agent name
        capturedVars["agentName"] = "Zest"
        val step1Response = substituteVariables(onboardingScriptSteps[0].responseTemplate!!, capturedVars)
        assertEquals(true, step1Response.contains("Zest"))
        assertEquals(false, step1Response.contains("{agentName}"))
        
        // Step 2: User provides user name  
        capturedVars["userName"] = "Alice"
        val step2Response = substituteVariables(onboardingScriptSteps[1].responseTemplate!!, capturedVars)
        assertEquals(true, step2Response.contains("Alice"))
        assertEquals(false, step2Response.contains("{userName}"))
        
        // Step 3: User provides style
        capturedVars["style"] = "casual"
        val step3Response = substituteVariables(onboardingScriptSteps[2].responseTemplate!!, capturedVars)
        assertEquals(true, step3Response.contains("Alice"))
        assertEquals(false, step3Response.contains("{userName}"))
        
        // Step 4: User provides boundaries
        capturedVars["boundaries"] = "ask before actions"
        val step4Response = substituteVariables(onboardingScriptSteps[3].responseTemplate!!, capturedVars)
        assertEquals(true, step4Response.contains("Zest"))
        assertEquals(true, step4Response.contains("Alice")) 
        assertEquals(true, step4Response.contains("casual"))
        assertEquals(false, step4Response.contains("{agentName}"))
        assertEquals(false, step4Response.contains("{userName}"))
        assertEquals(false, step4Response.contains("{style}"))
    }
    
    @Test
    fun profileGeneration_fromCapturedVariables() {
        // Test that the complete flow generates the correct profile
        val fullVariables = mapOf(
            "agentName" to "Zest",
            "userName" to "Alice",
            "style" to "casual and friendly",
            "boundaries" to "ask before major actions"
        )
        
        val profile = buildProfileFromCapturedVariables(fullVariables)
        
        // Verify all captured variables are correctly mapped
        assertEquals("Zest", profile.agentName)
        assertEquals("Alice", profile.userName)
        assertEquals("Alice", profile.userAddress)
        assertEquals("casual and friendly", profile.relationshipStyle)
        assertEquals("ask before major actions", profile.boundaries)
        
        // Verify defaults for non-captured fields
        assertEquals("citrus spirit", profile.agentNature)
        assertEquals("🍋", profile.agentEmoji) 
        assertEquals("chill but sharp", profile.agentVibe)
        assertEquals("onboarding completed", profile.userContext)
        assertEquals(1.0f, profile.confidence)
    }
}

// Helper functions need to be accessible for testing.
// These were added to OnboardingFlow.kt as private functions, but for testing
// we need them to be internal. This ensures they can be accessed by tests.
class CleanCapturedInputTest {
    @Test
    fun `strips My name is prefix`() {
        assertEquals("Joe", cleanCapturedInput("userName", "My name is Joe"))
    }

    @Test
    fun `strips Im prefix`() {
        assertEquals("Sarah", cleanCapturedInput("userName", "I'm Sarah"))
    }

    @Test
    fun `strips Call me prefix`() {
        assertEquals("Zest", cleanCapturedInput("agentName", "Call me Zest"))
    }

    @Test
    fun `preserves plain name`() {
        assertEquals("Joe", cleanCapturedInput("userName", "Joe"))
    }

    @Test
    fun `preserves non-name variables`() {
        assertEquals("My name is Joe", cleanCapturedInput("style", "My name is Joe"))
    }

    @Test
    fun `case insensitive prefix matching`() {
        assertEquals("Joe", cleanCapturedInput("userName", "MY NAME IS Joe"))
    }

    @Test
    fun `handles empty stripped result by keeping original`() {
        assertEquals("I'm", cleanCapturedInput("userName", "I'm"))
    }
}
