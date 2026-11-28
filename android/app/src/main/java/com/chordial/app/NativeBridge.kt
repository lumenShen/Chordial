package com.chordial.app

import android.app.Activity
import android.view.Surface

object NativeBridge {
    init {
        System.loadLibrary("chordial_android")
    }

    external fun setStorageDirs(filesDir: String, cacheDir: String)
    external fun start(activity: Activity)
    external fun surfaceCreated(surface: Surface)
    external fun surfaceChanged(surface: Surface, width: Int, height: Int, scale: Float)
    external fun surfaceDestroyed(surface: Surface)
    external fun frame()
    external fun touch(action: Int, pointerId: Int, x: Float, y: Float, pressure: Float)
    external fun key(keyCode: Int, action: Int, metaState: Int)
    external fun back()
    external fun resume()
    external fun pause()
    external fun imeInput(text: String, replacementStart: Int, replacementEnd: Int)
    external fun requestPermissions()
}
