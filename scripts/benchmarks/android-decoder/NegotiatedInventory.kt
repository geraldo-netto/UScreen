package com.blent.benchmark

import android.os.Build
import com.blent.DecoderCapabilities
import kotlinx.coroutines.runBlocking
import org.json.JSONObject

/** Read-only app_process probe of the production T478 inventory implementation. */
object NegotiatedInventory {
    @JvmStatic fun main(args: Array<String>) = runBlocking {
        require(args.size == 3) { "Expected width height fps" }
        val (width, height, fps) = args.map(String::toInt)
        require(width in 2..4096 && height in 2..4096 && fps in 10..90)
        val report = DecoderCapabilities.report(width, height, fps).put("protocol", 2).put("scope", "inventory")
        println("BLENT_NEGOTIATED_INVENTORY:" + JSONObject().put("sdk", Build.VERSION.SDK_INT)
            .put("fingerprint", Build.FINGERPRINT).put("capabilities", report))
    }
}
