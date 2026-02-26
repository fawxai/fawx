package ai.citros.core

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PrivacyListTest {

    @Test
    fun `empty list returns false for any package`() {
        val list = InMemoryPrivacyList()
        assertFalse(list.isPrivate("com.example.app"))
        assertTrue(list.getAll().isEmpty())
    }

    @Test
    fun `add and remove package`() {
        val list = InMemoryPrivacyList()

        list.add("com.bank.app")
        assertTrue(list.isPrivate("com.bank.app"))

        list.remove("com.bank.app")
        assertFalse(list.isPrivate("com.bank.app"))
    }

    @Test
    fun `multiple packages are independent`() {
        val list = InMemoryPrivacyList()

        list.add("com.bank.app")
        list.add("com.health.app")
        list.add("com.password.app")

        assertTrue(list.isPrivate("com.bank.app"))
        assertTrue(list.isPrivate("com.health.app"))
        assertTrue(list.isPrivate("com.password.app"))

        list.remove("com.health.app")

        assertTrue(list.isPrivate("com.bank.app"))
        assertFalse(list.isPrivate("com.health.app"))
        assertTrue(list.isPrivate("com.password.app"))
        assertEquals(setOf("com.bank.app", "com.password.app"), list.getAll())
    }

    @Test
    fun `null and empty package handling`() {
        val list = InMemoryPrivacyList()

        list.addNullable(null)
        list.add("")
        list.add("   ")
        list.add("com.valid.app")

        assertFalse(list.isPrivate(""))
        assertFalse(list.isPrivate("   "))
        assertFalse(list.isPrivate("com.invalid.app"))
        assertTrue(list.isPrivate("com.valid.app"))

        list.removeNullable(null)
        list.remove("")
        list.remove("   ")
        assertTrue(list.isPrivate("com.valid.app"))
    }

    private class InMemoryPrivacyList : PrivacyList {
        private val packages = linkedSetOf<String>()

        override fun isPrivate(packageName: String): Boolean {
            val normalized = packageName.trim()
            return normalized.isNotEmpty() && normalized in packages
        }

        override fun getAll(): Set<String> = packages.toSet()

        override fun add(packageName: String) {
            val normalized = packageName.trim()
            if (normalized.isNotEmpty()) packages.add(normalized)
        }

        override fun remove(packageName: String) {
            val normalized = packageName.trim()
            if (normalized.isNotEmpty()) packages.remove(normalized)
        }

        fun addNullable(packageName: String?) {
            if (packageName != null) add(packageName)
        }

        fun removeNullable(packageName: String?) {
            if (packageName != null) remove(packageName)
        }
    }
}
