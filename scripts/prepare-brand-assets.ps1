param(
    [string]$ProjectRoot = (Split-Path -Parent $PSScriptRoot)
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$brandDirectory = Join-Path $ProjectRoot 'public\brand'
[System.IO.Directory]::CreateDirectory($brandDirectory) | Out-Null

function Export-SquareLogo {
    param(
        [string]$Source,
        [string]$Destination,
        [int]$Size
    )

    $sourceImage = [System.Drawing.Bitmap]::FromFile($Source)
    $result = [System.Drawing.Bitmap]::new(
        $Size,
        $Size,
        [System.Drawing.Imaging.PixelFormat]::Format32bppArgb
    )
    $graphics = [System.Drawing.Graphics]::FromImage($result)
    try {
        $graphics.Clear([System.Drawing.Color]::Transparent)
        $graphics.CompositingMode = [System.Drawing.Drawing2D.CompositingMode]::SourceCopy
        $graphics.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
        $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
        $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
        $sourceRectangle = [System.Drawing.Rectangle]::new(420, 0, 1080, 1080)
        $destinationRectangle = [System.Drawing.Rectangle]::new(0, 0, $Size, $Size)
        $graphics.DrawImage(
            $sourceImage,
            $destinationRectangle,
            $sourceRectangle,
            [System.Drawing.GraphicsUnit]::Pixel
        )
        $result.Save($Destination, [System.Drawing.Imaging.ImageFormat]::Png)
    }
    finally {
        $graphics.Dispose()
        $result.Dispose()
        $sourceImage.Dispose()
    }
}

$whiteLogo = Join-Path $brandDirectory 'logo-white.png'
$blackLogo = Join-Path $brandDirectory 'logo-black.png'
Export-SquareLogo (Join-Path $ProjectRoot 'assets\brand\logo-white-source.png') $whiteLogo 512
Export-SquareLogo (Join-Path $ProjectRoot 'assets\brand\logo-black-source.png') $blackLogo 512

$logoImage = [System.Drawing.Bitmap]::FromFile($whiteLogo)
$icon = [System.Drawing.Bitmap]::new(
    1024,
    1024,
    [System.Drawing.Imaging.PixelFormat]::Format32bppArgb
)
$iconGraphics = [System.Drawing.Graphics]::FromImage($icon)
try {
    $iconGraphics.Clear([System.Drawing.Color]::Transparent)
    $iconGraphics.CompositingMode = [System.Drawing.Drawing2D.CompositingMode]::SourceCopy
    $iconGraphics.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
    $iconGraphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $iconGraphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $iconGraphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
    $iconGraphics.DrawImage($logoImage, 96, 96, 832, 832)
    $icon.Save(
        (Join-Path $brandDirectory 'app-icon.png'),
        [System.Drawing.Imaging.ImageFormat]::Png
    )
}
finally {
    $iconGraphics.Dispose()
    $icon.Dispose()
    $logoImage.Dispose()
}
