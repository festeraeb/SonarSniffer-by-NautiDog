/**
 * Logs payload delta for the next 50 pings:
 * delta_bytes = Field4(data_size) - bodyVarstructLength - Field7(sample_count)
 *
 * If delta_bytes > 0, flag the ping so UI can paint the "extra" zone in magenta.
 */
data class PingDiag(
    val seq: Int,
    val field4DataSize: Int,        // header field 4
    val bodyVarstructLen: Int,      // decoded body varstruct bytes
    val field7SampleCount: Int,     // body field 7 label
    val renderedWidth: Int          // actual decoded sample width (post unpack)
)

fun logPayloadDeltaNext50(pings: List<PingDiag>, tag: String = "RSD_DELTA") {
    pings.take(50).forEachIndexed { idx, p ->
        val payloadBytes = (p.field4DataSize - p.bodyVarstructLen).coerceAtLeast(0)
        val deltaBytes = payloadBytes - p.field7SampleCount
        val extraSamples = (p.renderedWidth - p.field7SampleCount).coerceAtLeast(0)
        val flag = if (deltaBytes > 0 || extraSamples > 0) "MAGENTA" else "OK"
        android.util.Log.i(
            tag,
            "i=$idx seq=${p.seq} payloadBytes=$payloadBytes field7=${p.field7SampleCount} " +
                "deltaBytes=$deltaBytes renderedWidth=${p.renderedWidth} extraSamples=$extraSamples flag=$flag"
        )
    }
}

/**
 * UI helper for high-contrast extra-byte overlay.
 * Paint x >= nominalSampleCount in magenta when payload is wider than Field 7.
 */
fun shouldPaintMagenta(x: Int, nominalSampleCount: Int, renderedWidth: Int): Boolean {
    if (nominalSampleCount <= 0 || renderedWidth <= nominalSampleCount) return false
    val splitX = ((nominalSampleCount.toDouble() / renderedWidth.toDouble()) * renderedWidth).toInt()
    return x >= splitX
}
