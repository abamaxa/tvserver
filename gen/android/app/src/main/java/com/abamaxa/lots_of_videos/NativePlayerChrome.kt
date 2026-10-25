package com.abamaxa.lots_of_videos

import android.content.Context
import android.content.res.ColorStateList
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.RippleDrawable
import android.os.Build
import android.text.TextUtils
import android.view.Gravity
import android.view.View
import android.widget.FrameLayout
import android.widget.ImageButton
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.RadioButton
import android.widget.SeekBar
import android.widget.Switch
import android.widget.TextView
import androidx.core.graphics.Insets
import org.json.JSONObject
import java.util.Locale

/** Presentation only: the Activity routes actions to its existing durable playback session. */
internal class NativePlayerChrome(context: Context, private val action: (Action) -> Unit) : FrameLayout(context) {
    enum class Action { BACK, STOP, TOGGLE, REWIND, FORWARD, PREVIOUS, NEXT, VIDEO, VISUALIZER, BACKGROUND, SETTINGS, AUDIO, SUBTITLES, SPEED }
    val progress = SeekBar(context)
    val backgroundAudio = Switch(context)
    var safeInsets: Insets = Insets.NONE
        private set
    private val header = LinearLayout(context)
    private val footer = LinearLayout(context)
    private val title = label(17f, WHITE, medium = true)
    private val collection = label(11f, MUTED)
    private val elapsed = label(12f, WHITE)
    private val duration = label(12f, MUTED)
    private val toggle: ImageButton
    private val previous: ImageButton
    private val next: ImageButton
    private val video: RadioButton
    private val visualizer: RadioButton
    private var updating = false
    private var insetsApplied = false

    init {
        // Empty space does not intercept taps on the video/visualizer underneath.
        header.orientation = LinearLayout.VERTICAL
        header.background = GradientDrawable(GradientDrawable.Orientation.TOP_BOTTOM,
            intArrayOf(0xD911151C.toInt(), 0x0011151C))
        val heading = LinearLayout(context).apply { gravity = Gravity.CENTER_VERTICAL }
        heading.addView(icon(NativePlayerIcon.Kind.BACK, "Back", Action.BACK), row(48, 48))
        val names = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(dp(8), 0, dp(16), 0)
            addView(title, LinearLayout.LayoutParams(-1, -2))
            addView(collection, LinearLayout.LayoutParams(-1, -2).apply { topMargin = dp(4) })
        }
        heading.addView(names, LinearLayout.LayoutParams(0, dp(56), 1f))
        heading.addView(icon(NativePlayerIcon.Kind.SETTINGS, "Playback settings", Action.SETTINGS), row(48, 48))
        heading.addView(icon(NativePlayerIcon.Kind.STOP, "Stop", Action.STOP), row(48, 48))
        header.addView(heading, LinearLayout.LayoutParams(-1, dp(64)))
        addView(header, LayoutParams(-1, dp(108), Gravity.TOP))

        val transport = LinearLayout(context).apply { gravity = Gravity.CENTER_VERTICAL }
        previous = icon(NativePlayerIcon.Kind.PREVIOUS, "Previous item", Action.PREVIOUS)
        transport.addView(previous, row(48, 56))
        transport.addView(icon(NativePlayerIcon.Kind.REWIND, "Back 10 seconds", Action.REWIND,
            size = 30, fill = 0x33000000), row(56, 56))
        toggle = icon(NativePlayerIcon.Kind.PAUSE, "Pause", Action.TOGGLE, size = 32, fill = WHITE, ink = INK)
        transport.addView(toggle, row(72, 72).apply { setMargins(dp(28), 0, dp(28), 0) })
        transport.addView(icon(NativePlayerIcon.Kind.FORWARD, "Forward 30 seconds", Action.FORWARD,
            size = 30, fill = 0x33000000), row(56, 56))
        next = icon(NativePlayerIcon.Kind.NEXT, "Next item", Action.NEXT)
        transport.addView(next, row(48, 56))
        previous.visibility = View.GONE
        next.visibility = View.GONE
        addView(transport, LayoutParams(-2, -2, Gravity.CENTER))

        footer.orientation = LinearLayout.VERTICAL
        footer.background = GradientDrawable(GradientDrawable.Orientation.BOTTOM_TOP,
            intArrayOf(0xED0A0E14.toInt(), 0xCC0A0E14.toInt(), 0x000A0E14))
        val timeline = LinearLayout(context).apply { gravity = Gravity.CENTER_VERTICAL }
        elapsed.gravity = Gravity.START or Gravity.CENTER_VERTICAL
        duration.gravity = Gravity.END or Gravity.CENTER_VERTICAL
        elapsed.typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
        elapsed.fontFeatureSettings = "tnum"
        duration.fontFeatureSettings = "tnum"
        timeline.addView(elapsed, row(58, 40))
        progress.apply {
            max = 1000
            contentDescription = "Video position"
            progressTintList = ColorStateList.valueOf(ACCENT)
            progressBackgroundTintList = ColorStateList.valueOf(0x4DFFFFFF)
            secondaryProgressTintList = ColorStateList.valueOf(0x66FFFFFF)
            thumb = shape(ACCENT, 50).apply { setSize(dp(12), dp(12)) }
            thumbOffset = dp(6)
            minHeight = dp(3)
            maxHeight = dp(3)
            splitTrack = false
            setPadding(dp(6), 0, dp(6), 0)
        }
        timeline.addView(progress, LinearLayout.LayoutParams(0, dp(40), 1f))
        timeline.addView(duration, row(58, 40))
        footer.addView(timeline, LinearLayout.LayoutParams(-1, dp(40)))

        val options = LinearLayout(context).apply { gravity = Gravity.CENTER_VERTICAL }
        val modes = LinearLayout(context).apply {
            gravity = Gravity.CENTER_VERTICAL
            background = shape(0x660D131C, 25, 0x24FFFFFF)
            setPadding(dp(3), dp(3), dp(3), dp(3))
        }
        video = mode("Video", NativePlayerIcon.Kind.VIDEO, Action.VIDEO)
        visualizer = mode("Visualizer", NativePlayerIcon.Kind.VISUALIZER, Action.VISUALIZER)
        modes.addView(video, row(104, 44))
        modes.addView(visualizer, row(125, 44))
        options.addView(modes, row(-2, 50))
        options.addView(View(context), LinearLayout.LayoutParams(0, 1, 1f))
        backgroundAudio.apply {
            text = "Screen-off audio"
            contentDescription = "Keep audio playing with screen off"
            textSize = 12f
            typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
            setTextColor(MUTED)
            gravity = Gravity.CENTER_VERTICAL
            setPadding(dp(14), 0, dp(14), 0)
            compoundDrawablePadding = dp(8)
            setCompoundDrawablesRelativeWithIntrinsicBounds(NativePlayerIcon(context, NativePlayerIcon.Kind.AUDIO, MUTED, 18), null, null, null)
            showText = false
            switchPadding = dp(12)
            switchMinWidth = dp(32)
            thumbDrawable = shape(WHITE, 50).apply { setSize(dp(18), dp(18)) }
            trackDrawable = shape(WHITE, 50).apply { setSize(dp(32), dp(14)) }
            val states = arrayOf(intArrayOf(android.R.attr.state_checked), intArrayOf())
            thumbTintList = ColorStateList(states, intArrayOf(ACCENT, 0xFFB5BDC9.toInt()))
            trackTintList = ColorStateList(states, intArrayOf(0xFF3D786D.toInt(), 0xFF414A57.toInt()))
            setOnCheckedChangeListener { _, _ -> if (!updating) action(Action.BACKGROUND) }
        }
        options.addView(backgroundAudio, row(-2, 48))
        footer.addView(options, LinearLayout.LayoutParams(-1, dp(50)))
        addView(footer, LayoutParams(-1, -2, Gravity.BOTTOM))
        setDisplayMode("video")
        applyInsets(Insets.NONE)
    }

    fun applyInsets(safe: Insets) {
        if (insetsApplied && safeInsets == safe) return
        insetsApplied = true
        safeInsets = safe
        header.setPadding(safe.left + dp(12), safe.top + dp(4), safe.right + dp(12), 0)
        header.layoutParams = header.layoutParams.apply { height = safe.top + dp(108) }
        footer.setPadding(safe.left + dp(24), dp(24), safe.right + dp(24), safe.bottom + dp(12))
    }

    fun bind(state: JSONObject, dragging: Boolean) {
        updating = true
        title.text = state.optString("video")
        val playlist = state.optString("playlistName", "")
        collection.text = if (playlist.isNotEmpty())
            "$playlist · ${state.optInt("queueIndex") + 1} of ${state.optInt("queueLength")}" else state.optString("collection")
        val queued = state.optString("playlistId", "").isNotEmpty()
        previous.visibility = if (queued) View.VISIBLE else View.GONE
        next.visibility = if (queued) View.VISIBLE else View.GONE
        previous.isEnabled = state.optBoolean("hasPrevious")
        next.isEnabled = state.optBoolean("hasNext")
        previous.alpha = if (previous.isEnabled) 1f else 0.35f
        next.alpha = if (next.isEnabled) 1f else 0.35f
        collection.visibility = if (collection.text.isNullOrBlank()) View.GONE else View.VISIBLE
        backgroundAudio.isChecked = state.optBoolean("backgroundAudio")
        backgroundAudio.setTextColor(if (backgroundAudio.isChecked) WHITE else MUTED)
        backgroundAudio.background = shape(if (backgroundAudio.isChecked) 0x333EA58F else 0x330D131C,
            24, if (backgroundAudio.isChecked) 0x6674DFC6 else 0x24FFFFFF)
        setDisplayMode(state.optString("displayMode", "video"))
        val time = (state.optDouble("currentTime") * 1000).toLong().coerceAtLeast(0)
        val length = (state.optDouble("duration") * 1000).toLong().coerceAtLeast(0)
        if (!dragging) progress.progress = if (length > 0) (time * 1000 / length).toInt() else 0
        elapsed.text = formatTime(time)
        duration.text = formatTime(length)
        val paused = state.optBoolean("paused", true)
        val description = if (paused) "Play" else "Pause"
        toggle.contentDescription = description
        if (Build.VERSION.SDK_INT >= 26) toggle.tooltipText = description
        toggle.setImageDrawable(NativePlayerIcon(context, if (paused) NativePlayerIcon.Kind.PLAY else NativePlayerIcon.Kind.PAUSE, INK, 32))
        updating = false
    }

    fun setDisplayMode(mode: String) {
        for ((button, selected, kind) in listOf(
            Triple(video, mode != "visualizer", NativePlayerIcon.Kind.VIDEO),
            Triple(visualizer, mode == "visualizer", NativePlayerIcon.Kind.VISUALIZER))) {
            button.isSelected = selected
            button.isChecked = selected
            button.setTextColor(if (selected) INK else MUTED)
            button.background = ripple(if (selected) ACCENT else Color.TRANSPARENT, 22)
            button.setCompoundDrawablesRelativeWithIntrinsicBounds(
                NativePlayerIcon(context, kind, if (selected) INK else MUTED, 17), null, null, null)
        }
    }

    fun settingsContent(audio: String, subtitles: String, speed: String, choose: (Action) -> Unit, close: () -> Unit): View {
        val panel = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(12), dp(8), dp(12), dp(12))
            background = shape(PANEL, 24, 0x24FFFFFF)
        }
        val heading = LinearLayout(context).apply { gravity = Gravity.CENTER_VERTICAL; setPadding(dp(12), 0, 0, 0) }
        heading.addView(label(16f, WHITE, true).apply { text = "Playback" }, LinearLayout.LayoutParams(0, dp(48), 1f))
        heading.addView(icon(NativePlayerIcon.Kind.CLOSE, "Close settings", null).apply {
            setOnClickListener { close() }
        }, row(48, 48))
        panel.addView(heading)
        for ((name, value, kind, selection) in listOf(
            Setting("Audio track", audio, NativePlayerIcon.Kind.AUDIO, Action.AUDIO),
            Setting("Subtitles", subtitles, NativePlayerIcon.Kind.SUBTITLES, Action.SUBTITLES),
            Setting("Playback speed", speed, NativePlayerIcon.Kind.SPEED, Action.SPEED))) {
            val item = LinearLayout(context).apply {
                gravity = Gravity.CENTER_VERTICAL
                background = ripple(Color.TRANSPARENT, 14)
                setPadding(dp(12), 0, dp(12), 0)
                isFocusable = true
                contentDescription = name
                setOnClickListener { choose(selection) }
            }
            item.addView(ImageView(context).apply { setImageDrawable(NativePlayerIcon(context, kind, MUTED, 22)) }, row(24, 24))
            val labels = LinearLayout(context).apply {
                orientation = LinearLayout.VERTICAL
                gravity = Gravity.CENTER_VERTICAL
                setPadding(dp(16), 0, dp(12), 0)
                addView(label(14f, WHITE).apply { text = name })
                addView(label(11f, MUTED).apply { text = value }, LinearLayout.LayoutParams(-1, -2).apply { topMargin = dp(4) })
            }
            item.addView(labels, LinearLayout.LayoutParams(0, -1, 1f))
            item.addView(ImageView(context).apply {
                setImageDrawable(NativePlayerIcon(context, NativePlayerIcon.Kind.CHEVRON, MUTED, 18))
            }, row(18, 18))
            panel.addView(item, LinearLayout.LayoutParams(-1, dp(64)))
        }
        return panel
    }

    private data class Setting(val name: String, val value: String, val kind: NativePlayerIcon.Kind, val selection: Action)

    private fun mode(name: String, kind: NativePlayerIcon.Kind, selection: Action) = RadioButton(context).apply {
        text = name
        textSize = 12f
        typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
        includeFontPadding = false
        buttonDrawable = null
        gravity = Gravity.CENTER
        setPadding(dp(14), 0, dp(14), 0)
        compoundDrawablePadding = dp(7)
        contentDescription = name
        isFocusable = true
        setOnClickListener { action(selection) }
        setCompoundDrawablesRelativeWithIntrinsicBounds(NativePlayerIcon(context, kind, WHITE, 17), null, null, null)
    }

    private fun icon(kind: NativePlayerIcon.Kind, description: String, selection: Action?, size: Int = 24,
        fill: Int = Color.TRANSPARENT, ink: Int = WHITE) = ImageButton(context).apply {
        contentDescription = description
        if (Build.VERSION.SDK_INT >= 26) tooltipText = description
        scaleType = ImageView.ScaleType.CENTER
        setImageDrawable(NativePlayerIcon(context, kind, ink, size))
        background = ripple(fill, 50)
        setPadding(0, 0, 0, 0)
        setOnClickListener { selection?.let(action) }
    }

    private fun label(size: Float, ink: Int, medium: Boolean = false) = TextView(context).apply {
        textSize = size
        setTextColor(ink)
        typeface = Typeface.create(if (medium) "sans-serif-medium" else "sans-serif", Typeface.NORMAL)
        maxLines = 1
        ellipsize = TextUtils.TruncateAt.END
        gravity = Gravity.CENTER_VERTICAL
        includeFontPadding = false
    }

    private fun row(width: Int, height: Int) = LinearLayout.LayoutParams(if (width < 0) width else dp(width), dp(height))
    fun dp(value: Int) = (value * resources.displayMetrics.density + 0.5f).toInt()
    private fun shape(fill: Int, radius: Int, border: Int = Color.TRANSPARENT) = GradientDrawable().apply {
        setColor(fill)
        cornerRadius = dp(radius).toFloat()
        if (border != Color.TRANSPARENT) setStroke(dp(1), border)
    }
    private fun ripple(fill: Int, radius: Int) = RippleDrawable(ColorStateList.valueOf(0x33FFFFFF),
        shape(fill, radius), shape(Color.WHITE, radius))

    private fun formatTime(ms: Long): String {
        val seconds = ms / 1000
        return if (seconds >= 3600) String.format(Locale.ROOT, "%d:%02d:%02d", seconds / 3600, seconds / 60 % 60, seconds % 60)
        else String.format(Locale.ROOT, "%d:%02d", seconds / 60, seconds % 60)
    }

    companion object {
        const val ACCENT = 0xFF8CE4CD.toInt()
        const val WHITE = 0xFFF3F7FB.toInt()
        const val MUTED = 0xFFB8C2CF.toInt()
        const val INK = 0xFF10231F.toInt()
        const val PANEL = 0xFF171F2B.toInt()
    }
}
