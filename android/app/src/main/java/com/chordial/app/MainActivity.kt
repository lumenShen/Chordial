package com.chordial.app

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.view.Choreographer
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.ViewGroup
import android.view.inputmethod.InputMethodManager
import android.widget.FrameLayout
import androidx.core.content.ContextCompat
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat

class MainActivity : Activity(), SurfaceHolder.Callback {
    private lateinit var root: FrameLayout
    private lateinit var surfaceView: SurfaceView
    private lateinit var imeProxy: ImeProxyView
    private val surfaceLocation = IntArray(2)
    private var resumed = false
    private var surfaceAlive = false
    private var frameScheduled = false
    private val choreographer = Choreographer.getInstance()
    private val frameCallback = object : Choreographer.FrameCallback {
        override fun doFrame(frameTimeNanos: Long) {
            frameScheduled = false
            if (surfaceAlive && resumed) {
                NativeBridge.frame()
                scheduleFrame()
            }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        WindowCompat.setDecorFitsSystemWindows(window, true)
        root = FrameLayout(this)
        surfaceView = SurfaceView(this)
        surfaceView.holder.addCallback(this)
        root.addView(
            surfaceView,
            FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT,
            ),
        )
        // 承载软键盘 InputConnection 的透明 1x1 视图。
        imeProxy = ImeProxyView(this)
        root.addView(imeProxy, FrameLayout.LayoutParams(1, 1))
        setContentView(root)
        ViewCompat.setOnApplyWindowInsetsListener(root) { view, insets ->
            val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars())
            // 键盘弹出时把内容顶上去：用 ime inset 扩展底部 padding，
            // 触发 surfaceChanged，gpui 会按新高度重排。
            val ime = insets.getInsets(WindowInsetsCompat.Type.ime())
            view.setPadding(bars.left, bars.top, bars.right, maxOf(bars.bottom, ime.bottom))
            insets
        }
        requestRuntimePermissions()
        startPlaybackService()
        NativeBridge.setStorageDirs(filesDir.absolutePath, cacheDir.absolutePath)
        NativeBridge.start(this)
    }

    override fun onResume() {
        super.onResume()
        resumed = true
        NativeBridge.resume()
        scheduleFrame()
    }

    override fun onPause() {
        resumed = false
        NativeBridge.pause()
        if (frameScheduled) {
            choreographer.removeFrameCallback(frameCallback)
            frameScheduled = false
        }
        super.onPause()
    }

    override fun onDestroy() {
        if (frameScheduled) {
            choreographer.removeFrameCallback(frameCallback)
            frameScheduled = false
        }
        super.onDestroy()
    }

    override fun surfaceCreated(holder: SurfaceHolder) {
        surfaceAlive = true
        NativeBridge.surfaceCreated(holder.surface)
        scheduleFrame()
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        NativeBridge.surfaceChanged(holder.surface, width, height, resources.displayMetrics.density)
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        surfaceAlive = false
        NativeBridge.surfaceDestroyed(holder.surface)
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (event.pointerCount == 0) return super.onTouchEvent(event)
        val action = event.actionMasked
        val pointerId = event.getPointerId(0)
        // MotionEvent 坐标相对 Activity 窗口；gpui 的窗口原点在 SurfaceView
        // 左上角（存在状态栏Insets偏移），先换算到 Surface 坐标系。
        surfaceView.getLocationInWindow(surfaceLocation)
        NativeBridge.touch(
            action,
            pointerId,
            event.getX(0) - surfaceLocation[0],
            event.getY(0) - surfaceLocation[1],
            event.getPressure(0),
        )
        return true
    }

    override fun dispatchKeyEvent(event: KeyEvent): Boolean {
        // 音量键交给系统（AudioService 调整媒体音量），否则会被 gpui 吞掉。
        if (event.keyCode == KeyEvent.KEYCODE_VOLUME_UP ||
            event.keyCode == KeyEvent.KEYCODE_VOLUME_DOWN
        ) {
            return super.dispatchKeyEvent(event)
        }
        if (event.keyCode == KeyEvent.KEYCODE_BACK) {
            if (event.action == KeyEvent.ACTION_DOWN) NativeBridge.back()
            return true
        }
        android.util.Log.d("ChordialKey", "code=${event.keyCode} action=${event.action}")
        NativeBridge.key(event.keyCode, event.action, event.metaState)
        return true
    }

    @Suppress("DEPRECATION")
    override fun onBackPressed() {
        NativeBridge.back()
    }

    /** 由 Rust 侧（gpui `update_ime_position` / 失焦帧）调用。 */
    fun setImeVisible(visible: Boolean) {
        runOnUiThread {
            android.util.Log.d("ChordialIme", "visible=$visible")
            val imm = getSystemService(InputMethodManager::class.java) ?: return@runOnUiThread
            if (visible) {
                imeProxy.requestFocus()
                imeProxy.post {
                    if (Build.VERSION.SDK_INT >= 30) {
                        // showSoftInput 在 focus 尚未稳定时会静默失败；
                        // API 30+ 用 WindowInsetsController 更可靠。
                        WindowInsetsControllerCompat(window, imeProxy)
                            .show(WindowInsetsCompat.Type.ime())
                    } else {
                        imm.showSoftInput(imeProxy, InputMethodManager.SHOW_IMPLICIT)
                    }
                }
            } else {
                if (Build.VERSION.SDK_INT >= 30) {
                    WindowInsetsControllerCompat(window, imeProxy)
                        .hide(WindowInsetsCompat.Type.ime())
                } else {
                    imm.hideSoftInputFromWindow(imeProxy.windowToken, 0)
                }
            }
        }
    }

    private fun requestRuntimePermissions() {
        val permissions = buildList {
            if (Build.VERSION.SDK_INT >= 33) {
                add(Manifest.permission.READ_MEDIA_AUDIO)
                add(Manifest.permission.POST_NOTIFICATIONS)
            } else {
                add(Manifest.permission.READ_EXTERNAL_STORAGE)
            }
        }
        val missing = permissions.filter {
            checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED
        }
        if (missing.isNotEmpty()) {
            requestPermissions(missing.toTypedArray(), PERMISSION_REQUEST)
        }
    }

    /** 后台播放保活：前台服务在应用退到后台后维持进程与音频线程。 */
    private fun startPlaybackService() {
        val intent = Intent(this, PlaybackService::class.java)
        ContextCompat.startForegroundService(this, intent)
    }

    private fun scheduleFrame() {
        if (!frameScheduled && surfaceAlive && resumed) {
            frameScheduled = true
            choreographer.postFrameCallback(frameCallback)
        }
    }

    private companion object {
        const val PERMISSION_REQUEST = 100
    }
}
