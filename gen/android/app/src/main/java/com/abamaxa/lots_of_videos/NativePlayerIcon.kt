package com.abamaxa.lots_of_videos

import android.content.Context
import android.graphics.Canvas
import android.graphics.ColorFilter
import android.graphics.Paint
import android.graphics.Path
import android.graphics.PixelFormat
import android.graphics.RectF
import android.graphics.Typeface
import android.graphics.drawable.Drawable
import kotlin.math.cos
import kotlin.math.sin

/** Small vector icons that stay crisp without introducing another UI or playback dependency. */
internal class NativePlayerIcon(
    context: Context,
    private val kind: Kind,
    private val color: Int = 0xFFF5F7FA.toInt(),
    size: Int = 24,
) : Drawable() {
    enum class Kind { BACK, PLAY, PAUSE, STOP, REWIND, FORWARD, PREVIOUS, NEXT, SETTINGS, VIDEO, VISUALIZER, AUDIO, SUBTITLES, SPEED, CLOSE, CHEVRON }
    private val pixels = (size * context.resources.displayMetrics.density).toInt()
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        strokeWidth = 1.8f
        strokeCap = Paint.Cap.ROUND
        strokeJoin = Paint.Join.ROUND
    }

    override fun getIntrinsicWidth() = pixels
    override fun getIntrinsicHeight() = pixels
    override fun getOpacity() = PixelFormat.TRANSLUCENT
    override fun setAlpha(alpha: Int) { paint.alpha = alpha; invalidateSelf() }
    override fun setColorFilter(filter: ColorFilter?) { paint.colorFilter = filter; invalidateSelf() }

    override fun draw(canvas: Canvas) {
        val save = canvas.save()
        canvas.translate(bounds.left.toFloat(), bounds.top.toFloat())
        canvas.scale(bounds.width() / 24f, bounds.height() / 24f)
        paint.color = color
        paint.style = Paint.Style.STROKE
        fun line(vararg points: Float) {
            val path = Path().apply {
                moveTo(points[0], points[1])
                for (i in 2 until points.size step 2) lineTo(points[i], points[i + 1])
            }
            canvas.drawPath(path, paint)
        }
        fun fill(path: Path) {
            paint.style = Paint.Style.FILL
            canvas.drawPath(path, paint)
            paint.style = Paint.Style.STROKE
        }
        when (kind) {
            Kind.BACK -> { line(13f, 5f, 6f, 12f, 13f, 19f); line(6f, 12f, 21f, 12f) }
            Kind.PLAY -> fill(Path().apply { moveTo(8f, 4f); lineTo(20f, 12f); lineTo(8f, 20f); close() })
            Kind.PREVIOUS, Kind.NEXT -> {
                if (kind == Kind.PREVIOUS) canvas.scale(-1f, 1f, 12f, 12f)
                fill(Path().apply { moveTo(5f, 5f); lineTo(16f, 12f); lineTo(5f, 19f); close() })
                line(19f, 5f, 19f, 19f)
            }
            Kind.PAUSE -> {
                paint.style = Paint.Style.FILL
                canvas.drawRoundRect(6f, 4f, 10f, 20f, 1f, 1f, paint)
                canvas.drawRoundRect(14f, 4f, 18f, 20f, 1f, 1f, paint)
            }
            Kind.STOP -> {
                paint.style = Paint.Style.FILL
                canvas.drawRoundRect(5f, 5f, 19f, 19f, 2f, 2f, paint)
            }
            Kind.REWIND, Kind.FORWARD -> {
                if (kind == Kind.REWIND) canvas.scale(-1f, 1f, 12f, 12f)
                canvas.drawArc(RectF(3f, 4f, 21f, 22f), -90f, 295f, false, paint)
                line(9f, 1.5f, 12f, 4f, 9f, 7f)
                if (kind == Kind.REWIND) canvas.scale(-1f, 1f, 12f, 12f)
                paint.style = Paint.Style.FILL
                paint.textAlign = Paint.Align.CENTER
                paint.typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
                paint.textSize = 8.5f
                canvas.drawText(if (kind == Kind.REWIND) "10" else "30", 12f, 16f, paint)
            }
            Kind.SETTINGS -> {
                val gear = Path()
                for (i in 0 until 32) {
                    val angle = (i * Math.PI / 16 - Math.PI / 8)
                    val radius = if (i % 4 in 1..2) 9.5 else 7.6
                    val x = (12 + cos(angle) * radius).toFloat()
                    val y = (12 + sin(angle) * radius).toFloat()
                    if (i == 0) gear.moveTo(x, y) else gear.lineTo(x, y)
                }
                gear.close()
                canvas.drawPath(gear, paint)
                canvas.drawCircle(12f, 12f, 3f, paint)
            }
            Kind.VIDEO -> {
                canvas.drawRoundRect(2f, 5f, 22f, 19f, 3f, 3f, paint)
                fill(Path().apply { moveTo(10f, 8f); lineTo(16f, 12f); lineTo(10f, 16f); close() })
            }
            Kind.VISUALIZER -> {
                for ((x, height) in listOf(4f to 3f, 8f to 6f, 12f to 9f, 16f to 5f, 20f to 2f))
                    canvas.drawLine(x, 12 - height, x, 12 + height, paint)
            }
            Kind.AUDIO -> {
                canvas.drawArc(RectF(4f, 3f, 20f, 19f), 180f, 180f, false, paint)
                canvas.drawRoundRect(3f, 11f, 7f, 20f, 2f, 2f, paint)
                canvas.drawRoundRect(17f, 11f, 21f, 20f, 2f, 2f, paint)
            }
            Kind.SUBTITLES -> {
                canvas.drawRoundRect(2f, 4f, 22f, 20f, 3f, 3f, paint)
                paint.style = Paint.Style.FILL
                paint.typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
                paint.textSize = 9f
                paint.textAlign = Paint.Align.CENTER
                canvas.drawText("CC", 12f, 15.5f, paint)
            }
            Kind.SPEED -> {
                canvas.drawArc(RectF(3f, 3f, 21f, 21f), 135f, 270f, false, paint)
                line(12f, 12f, 17f, 7f)
                canvas.drawCircle(12f, 12f, 1.5f, paint)
            }
            Kind.CLOSE -> { line(6f, 6f, 18f, 18f); line(18f, 6f, 6f, 18f) }
            Kind.CHEVRON -> line(9f, 6f, 15f, 12f, 9f, 18f)
        }
        canvas.restoreToCount(save)
    }
}
