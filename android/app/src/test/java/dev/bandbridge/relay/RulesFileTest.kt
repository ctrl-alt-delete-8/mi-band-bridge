package dev.bandbridge.relay

import org.junit.Assert.assertEquals
import org.junit.Test

class RulesFileTest {
    private val three = listOf(
        Rule("bb01", "Page down", "3f2a9c1e-7b4d-4e0a-9c61-2d8e5f0a1b3c"),
        Rule("bb01", "Next slide", "8d7b2e4a-1c69-4f03-a512-9e0b6c3d8f21"),
        Rule("bb02", "Page up", "6a4c0d92-8f31-4b7e-932a-5d1e0c8b6f47"),
    )
    private val at = java.time.Instant.parse("2026-09-27T09:42:10Z")

    @Test
    fun exportThenImportGivesTheSameRules() {
        assertEquals(RulesFile.Read.Rules(three), RulesFile.read(RulesFile.write(three, at)))
    }

    /** The note fixes the keys, and keyMapId is the file's name for keyMapUid. */
    @Test
    fun theFileHasTheNotesKeys() {
        val json = org.json.JSONObject(RulesFile.write(three.take(1), at))
        assertEquals("bandbridge-relay-rules", json.getString("format"))
        assertEquals(1, json.getInt("version"))
        assertEquals("2026-09-27T09:42:10Z", json.getString("exportedAt"))
        val rule = json.getJSONArray("rules").getJSONObject(0)
        assertEquals(setOf("signal", "name", "keyMapId"), rule.keys().asSequence().toSet())
        assertEquals("3f2a9c1e-7b4d-4e0a-9c61-2d8e5f0a1b3c", rule.getString("keyMapId"))
    }

    @Test
    fun aFileThatIsNotTheFormatImportsNothing() {
        for (bad in listOf("", "not json", "[]", "{}",
            """{"format":"something-else","version":1,"rules":[]}""",
            """{"format":"bandbridge-relay-rules","version":2,"rules":[]}""",
            """{"format":"bandbridge-relay-rules","version":1,"rules":{}}""")) {
            assertEquals(bad, RulesFile.Read.NotARulesFile, RulesFile.read(bad))
        }
    }

    @Test
    fun aBrokenRuleIsSkippedAndTheRestAreKept() {
        val text = """{"format":"bandbridge-relay-rules","version":1,"rules":[
            {"signal":"bb01","name":"Page down","keyMapId":"a"},
            {"signal":"bb02","name":" ","keyMapId":"b"},
            {"signal":"bb03","keyMapId":"c"},
            "not an object",
            {"signal":" BB04 ","name":" Previous slide ","keyMapId":" d "},
            {"signal":"C0FFEE","name":"Odd","keyMapId":"e"}]}"""
        assertEquals(
            RulesFile.Read.Rules(listOf(
                Rule("bb01", "Page down", "a"),
                Rule("bb04", "Previous slide", "d"),
                Rule("c0ffee", "Odd", "e"),
            )),
            RulesFile.read(text),
        )
    }

    @Test
    fun aHugeFileIsNotARulesFile() {
        val huge = java.io.ByteArrayInputStream(ByteArray(RulesFile.MAX_BYTES + 1) { ' '.code.toByte() })
        assertEquals(RulesFile.Read.NotARulesFile, RulesFile.read(huge))
    }

    @Test
    fun importingTheSameRulesAgainAddsNothing() {
        assertEquals(three, RulesFile.merged(three, three))
    }

    /** Import only adds: what is there stays first and unchanged, and a copy inside the file counts once. */
    @Test
    fun importAddsNewRulesAtTheEndInFileOrder() {
        val incoming = listOf(
            Rule("bb03", "Play or pause", "c41e8a07"),
            Rule("bb01", "Page down", "3f2a9c1e-7b4d-4e0a-9c61-2d8e5f0a1b3c"),
            Rule("bb04", "Previous slide", "0b9d3f6e"),
            Rule("bb03", "Play or pause", "c41e8a07"),
        )
        assertEquals(three + listOf(Rule("bb03", "Play or pause", "c41e8a07"), Rule("bb04", "Previous slide", "0b9d3f6e")),
            RulesFile.merged(three, incoming))
    }

    @Test
    fun theDefaultFileNameCarriesTheDate() {
        assertEquals("mi-band-bridge-rules-2026-09-27.json", RulesFile.fileName(java.time.LocalDate.of(2026, 9, 27)))
    }

    /**
     * An import opens any file, of any type and any name, and reads what is
     * in it: a file named before 2.4.0, such as bandbridge-rules-2026-09-27.json,
     * is read as well as one named after.
     */
    @Test
    fun anImportOpensAnyFile() {
        assertEquals("*/*", RulesFile.OPEN_TYPE)
    }

    /**
     * The limit counts bytes: a rules file of exactly 1 MB is read, and one
     * byte more makes it something else, however good the rest of it is.
     */
    @Test
    fun theLimitIsExactlyOneMegabyte() {
        val text = RulesFile.write(three, at)
        val atTheLimit = text + " ".repeat(RulesFile.MAX_BYTES - text.toByteArray().size)
        assertEquals(RulesFile.MAX_BYTES, atTheLimit.toByteArray().size)
        assertEquals(RulesFile.Read.Rules(three), RulesFile.read(java.io.ByteArrayInputStream(atTheLimit.toByteArray())))
        assertEquals(RulesFile.Read.NotARulesFile, RulesFile.read(java.io.ByteArrayInputStream("$atTheLimit ".toByteArray())))
    }

    /** A long file is never read to its end: one byte past the limit is all it takes to know. */
    @Test(timeout = 10_000)
    fun aLongFileIsReadNoFurtherThanTheLimit() {
        var served = 0L
        val endless = object : java.io.InputStream() {
            override fun read(): Int {
                served++
                return ' '.code
            }
        }
        assertEquals(RulesFile.Read.NotARulesFile, RulesFile.read(endless))
        assertEquals(RulesFile.MAX_BYTES + 1L, served)
    }

    /**
     * A small hostile file of brackets. org.json reads a nested list by
     * calling itself, once per bracket, on the window's thread. The JVM's
     * org.json turns its own stack overflow into a JSONException, and
     * Android's does not, so the emulator imports such a file too.
     */
    @Test
    fun aDeeplyNestedFileIsNotARulesFile() {
        val deep = """{"format":"bandbridge-relay-rules","version":1,"rules":""" + "[".repeat(500_000)
        assertEquals(RulesFile.Read.NotARulesFile, RulesFile.read(deep))
    }

    /** B4: a rules file is one JSON object, so anything but blank space after it makes the file something else. */
    @Test
    fun somethingAfterTheObjectIsNotARulesFile() {
        val file = RulesFile.write(three, at)
        assertEquals(RulesFile.Read.NotARulesFile, RulesFile.read(file + """{"format":"bandbridge-relay-rules"}"""))
        assertEquals(RulesFile.Read.Rules(three), RulesFile.read(file + "\n\n  "))
    }

    /** B4: a signal, a name and a key map id must be text. A number or a null is not, even where it would read as one. */
    @Test
    fun aFieldThatIsNotTextIsSkipped() {
        val text = """{"format":"bandbridge-relay-rules","version":1,"rules":[
            {"signal":"bb01","name":1,"keyMapId":"a"},
            {"signal":"bb01","name":"Page down","keyMapId":null},
            {"signal":["bb01"],"name":"Page down","keyMapId":"b"},
            {"signal":"bb02","name":"Page up","keyMapId":"c"}]}"""
        assertEquals(RulesFile.Read.Rules(listOf(Rule("bb02", "Page up", "c"))), RulesFile.read(text))
    }

    /** Some Windows editors start a UTF-8 file with a byte order mark when they save it. */
    @Test
    fun aByteOrderMarkIsIgnored() {
        val marked = byteArrayOf(0xEF.toByte(), 0xBB.toByte(), 0xBF.toByte()) + RulesFile.write(three, at).toByteArray()
        assertEquals(RulesFile.Read.Rules(three), RulesFile.read(java.io.ByteArrayInputStream(marked)))
    }

    /** The file is bytes in UTF-8, and a name keeps every character through it, quotes and slashes too. */
    @Test
    fun aNameKeepsEveryCharacterThroughTheFile() {
        val odd = listOf(Rule("bb05", "Página ↓, \"next\" / back \\ 📖", "uid-5"))
        val bytes = RulesFile.write(odd, at).toByteArray(Charsets.UTF_8)
        assertEquals(RulesFile.Read.Rules(odd), RulesFile.read(java.io.ByteArrayInputStream(bytes)))
    }

    /** B4: a copy is judged after the trimming and normalizing that reading does, so case and spaces do not make a rule new. */
    @Test
    fun aCopyWithOtherCaseAndSpacesAddsNothing() {
        val copy = Rule(" BB01 ", " Page down ", " 3f2a9c1e-7b4d-4e0a-9c61-2d8e5f0a1b3c ")
        assertEquals(three, RulesFile.merged(three, listOf(copy)))
    }

    /**
     * 1 MB holds some 20,000 short rules, and a list that long ran the
     * window out of memory at every start. A file of more rules than the
     * list ever takes is no file this app wrote.
     */
    @Test
    fun aFileOfMoreRulesThanTheListTakesIsNotARulesFile() {
        fun file(count: Int) = RulesFile.write((1..count).map { Rule("bb01", "Rule $it", "uid-$it") }, at)
        val full = RulesFile.read(file(RulesFile.MAX_RULES))
        assertEquals(RulesFile.MAX_RULES, (full as RulesFile.Read.Rules).rules.size)
        assertEquals(RulesFile.Read.NotARulesFile, RulesFile.read(file(RulesFile.MAX_RULES + 1)))
    }

    /** FE-004: each of the three fields may hold 100 characters, counted after trimming. */
    @Test
    fun aFieldOf100CharactersIsKept() {
        val hundred = "c".repeat(100)
        val text = rulesFile(
            entry(" $hundred ", "Odd signal", "id-1"),
            entry("bb01", " ${"n".repeat(100)} ", "id-2"),
            entry("bb01", "Long id", " ${"i".repeat(100)} "),
        )
        assertEquals(
            RulesFile.Read.Rules(listOf(
                Rule(hundred, "Odd signal", "id-1"),
                Rule("bb01", "n".repeat(100), "id-2"),
                Rule("bb01", "Long id", "i".repeat(100)),
            )),
            RulesFile.read(text),
        )
    }

    /**
     * FE-004: one character more in any of the three, and the entry is
     * skipped as a broken one is, while the rest of the file is kept. One
     * name of 1,000,000 characters hung every start of the app, and one id
     * of 600,000 ended the relay at every press.
     */
    @Test
    fun anEntryWithAFieldOf101CharactersIsSkipped() {
        val text = rulesFile(
            entry("c".repeat(101), "Odd signal", "id-1"),
            entry("bb01", "n".repeat(101), "id-2"),
            entry("bb01", "Long id", "i".repeat(101)),
            entry("bb02", "Page up", "id-4"),
        )
        assertEquals(RulesFile.Read.Rules(listOf(Rule("bb02", "Page up", "id-4"))), RulesFile.read(text))
    }

    /** Import stops adding when the list is full, so no number of files can make it too long to draw. */
    @Test
    fun importStopsAddingWhenTheListIsFull() {
        val almost = (1 until RulesFile.MAX_RULES).map { Rule("bb01", "Rule $it", "uid-$it") }
        val incoming = listOf(Rule("bb02", "New one", "uid-a"), Rule("bb02", "New two", "uid-b"))
        val full = almost + incoming.first()
        assertEquals(full, RulesFile.merged(almost, incoming))
        assertEquals(full, RulesFile.merged(full, incoming))
    }

    /** A rules file of [entries], as JSON text. */
    private fun rulesFile(vararg entries: String) =
        """{"format":"bandbridge-relay-rules","version":1,"rules":[${entries.joinToString(",")}]}"""

    /** One entry of a rules file. The values are plain ASCII with no quotes, so they need no escaping. */
    private fun entry(signal: String, name: String, keyMapId: String) =
        """{"signal":"$signal","name":"$name","keyMapId":"$keyMapId"}"""
}
