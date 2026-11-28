package com.chordial.app

import android.content.Context
import android.text.InputType
import android.view.KeyEvent
import android.view.View
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection

/**
 * 软键盘代理视图。
 *
 * gpui 的文本输入由 Rust 维护，Android 壳只负责把 IME 的提交/删除/按键转给
 * `NativeBridge`。这里用 1x1 透明 View 承载 InputConnection：
 * - `commitText` → `imeInput(..., -1, -1)`（range=None 表示替换 gpui 当前选区）；
 * - `deleteSurroundingText` → 合成 DEL / FORWARD_DEL 按键，复用 gpui 的删除逻辑；
 * - 其它按键（回车/方向键）→ `NativeBridge.key`。
 *
 * 拼音等组词过程只显示在输入法自己的候选栏，不向 gpui 发送半成品文本。
 */
class ImeProxyView(context: Context) : View(context) {
    init {
        isFocusable = true
        isFocusableInTouchMode = true
        // 避免被无障碍服务当成可见控件；尺寸由调用方设为 1x1。
        importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
    }

    override fun onCreateInputConnection(outAttrs: EditorInfo): InputConnection {
        outAttrs.inputType = InputType.TYPE_CLASS_TEXT or
            InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
        outAttrs.imeOptions = EditorInfo.IME_ACTION_DONE or
            EditorInfo.IME_FLAG_NO_FULLSCREEN or
            EditorInfo.IME_FLAG_NO_EXTRACT_UI

        return object : BaseInputConnection(this, false) {
            override fun commitText(text: CharSequence?, newCursorPosition: Int): Boolean {
                if (!text.isNullOrEmpty()) {
                    NativeBridge.imeInput(text.toString(), -1, -1)
                }
                return true
            }

            override fun setComposingText(text: CharSequence?, newCursorPosition: Int): Boolean {
                // 组词中的预览不发给 gpui，等 commitText 一次提交。
                return true
            }

            override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
                repeat(beforeLength.coerceAtLeast(0)) { sendKey(KeyEvent.KEYCODE_DEL) }
                repeat(afterLength.coerceAtLeast(0)) { sendKey(KeyEvent.KEYCODE_FORWARD_DEL) }
                return true
            }

            override fun sendKeyEvent(event: KeyEvent): Boolean {
                NativeBridge.key(event.keyCode, event.action, event.metaState)
                return true
            }

            private fun sendKey(keyCode: Int) {
                NativeBridge.key(keyCode, KeyEvent.ACTION_DOWN, 0)
                NativeBridge.key(keyCode, KeyEvent.ACTION_UP, 0)
            }
        }
    }
}
