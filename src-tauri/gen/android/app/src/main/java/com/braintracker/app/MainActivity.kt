package com.braintracker.app

import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    askForLocalNetwork()
  }

  /**
   * Sync talks to the PC on the home network. From Android 17 (API 37) that needs the
   * "local network" permission (shown as Nearby devices); without it every connection to the
   * PC just times out. Asked for once; if it's refused, the app's sync error says where to allow it.
   */
  private fun askForLocalNetwork() {
    if (Build.VERSION.SDK_INT < 37) return
    val permission = "android.permission.ACCESS_LOCAL_NETWORK"
    if (checkSelfPermission(permission) != PackageManager.PERMISSION_GRANTED) {
      requestPermissions(arrayOf(permission), 4781)
    }
  }
}
