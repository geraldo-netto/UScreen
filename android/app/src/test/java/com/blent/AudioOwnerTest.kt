package com.blent

import android.Manifest
import kotlinx.coroutines.*
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.*
import org.robolectric.annotation.Config
import java.net.ServerSocket

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [27, 34])
class AudioOwnerTest {
    @Test fun t718_defaultOwnerRequestsPermissionAndRetiresBackgroundService() = runBlocking {
        val app = RuntimeEnvironment.getApplication()
        AudioOwner.reset()
        try {
            var prompts = 0; AudioOwner.permissionRequest = { prompts++ }
            assertNotNull(AudioOwner.permissionRequest)
            val binding = AudioOwner.get(app); assertSame(binding, AudioOwner.get(app))
            binding.start(); org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
            AudioInvitations.microphone.value = AudioEndpoint("a".repeat(64), 12345, 1, 1, 40, false)
            org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
            assertEquals(1, prompts); binding.permissionResult(false)
            Shadows.shadowOf(app).grantPermissions(Manifest.permission.RECORD_AUDIO)
            binding.configure(AudioPreferences(background = true, builtIn = false))
            ServerSocket(0).use { server ->
                AudioInvitations.microphone.value = AudioEndpoint("a".repeat(64), server.localPort, 1, 1, 40, true)
                Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
                withContext(Dispatchers.IO) { server.soTimeout = 2000; server.accept().use { socket ->
                    socket.soTimeout = 2000; assertEquals(76, socket.getInputStream().readNBytes(76).size)
                } }
                AudioOwner.stopSharing(); assertFalse(binding.sharing)
            }
            AudioOwner.stopped("old"); assertFalse(AudioOwner.owns("old"))
        } finally { AudioOwner.reset(); Shadows.shadowOf(android.os.Looper.getMainLooper()).idle() }
    }
}
