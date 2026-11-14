package app.stafy.player.media

import android.bluetooth.BluetoothClass
import android.bluetooth.BluetoothDevice
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log
import androidx.core.content.IntentCompat
import androidx.media3.common.util.UnstableApi

/**
 * Wakes the media service when the phone connects to a car's Bluetooth, so "Start playing when
 * the car connects" works after the car was off and Android stopped Catlify. Without a running
 * session the head unit's PLAY (Mazda Connect sends one on connect) reached no one until the
 * app was opened.
 *
 * ACL_CONNECTED is one of the implicit broadcasts a manifest receiver may still get, and being
 * a Bluetooth broadcast it lets the app start its service from the background. Needs
 * BLUETOOTH_CONNECT on Android 12+ (asked for in MainActivity).
 */
@UnstableApi
class CarConnectReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != BluetoothDevice.ACTION_ACL_CONNECTED) return
        val device = IntentCompat.getParcelableExtra(intent, BluetoothDevice.EXTRA_DEVICE, BluetoothDevice::class.java)
        if (!isCar(device)) return
        try {
            context.startService(
                Intent(context, StafyMediaService::class.java).setAction(StafyMediaService.ACTION_CAR_CONNECTED)
            )
        } catch (e: Exception) {
            // Background start refused (OEM battery restrictions): the head unit's PLAY still
            // reaches MediaButtonReceiver if Android routes it there.
            Log.w("Catlify", "couldn't start the media service for the car", e)
        }
    }

    private fun isCar(device: BluetoothDevice?): Boolean {
        val cls = try {
            device?.bluetoothClass
        } catch (e: SecurityException) {
            null
        } ?: return false
        // Head units report "car audio" or "hands-free"; earbuds and speakers don't.
        return cls.deviceClass == BluetoothClass.Device.AUDIO_VIDEO_CAR_AUDIO ||
            cls.deviceClass == BluetoothClass.Device.AUDIO_VIDEO_HANDSFREE
    }
}
