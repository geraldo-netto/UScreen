package com.uscreen

import android.app.Activity
import android.content.Intent
import android.os.Bundle

/** Token delivery gate protected by the manifest's shell-only DUMP permission. */
class TokenActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (TokenDelivery.store(this, intent)) {
            startActivity(Intent(this, MainActivity::class.java)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP))
        }
        finish()
    }
}
