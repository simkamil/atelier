# SPDX-License-Identifier: AGPL-3.0-or-later
[CmdletBinding()]
param(
    [ValidateSet('Availability', 'Capture')]
    [string] $Mode = 'Availability',
    [switch] $AvailabilityOnly,
    [string] $OutputJson = (Join-Path (Split-Path $PSScriptRoot -Parent) '_bmad-output/implementation-artifacts/us-210-word-reference.json'),
    [string] $SyntheticDocx = (Join-Path (Split-Path $PSScriptRoot -Parent) 'tests/text-layout-reference/generated/us-210-synthetic-corpus.docx')
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-WordComType {
    try {
        return [type]::GetTypeFromProgID('Word.Application')
    }
    catch {
        return $null
    }
}

function Write-JsonResult($Value, [string] $Path) {
    $json = ConvertTo-Json -InputObject $Value -Depth 12
    if ($Path) {
        $parent = Split-Path -Parent $Path
        if ($parent -and -not (Test-Path -LiteralPath $parent)) {
            New-Item -ItemType Directory -Path $parent -Force | Out-Null
        }
        # CreateNew also protects against an existing file appearing after validation.
        $stream = [System.IO.File]::Open($Path, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::Write)
        try {
            $bytes = [System.Text.UTF8Encoding]::new($false).GetBytes($json + "`n")
            $stream.Write($bytes, 0, $bytes.Length)
        }
        finally { $stream.Dispose() }
    }
    Write-Output $json
}

function Get-AvailabilityResult {
    $wordType = Get-WordComType
    $available = $null -ne $wordType
    return [ordered]@{
        schemaVersion = 1
        story = 'US-210'
        collectedAtUtc = [DateTime]::UtcNow.ToString('o')
        status = $(if ($available) { 'available' } else { 'noWord' })
        comProgId = 'Word.Application'
        comRegistered = $available
        oracleStarted = $false
        syntheticDocxCreated = $false
        layoutResults = $null
        note = $(if ($available) { 'Word COM is registered. Run with -Mode Capture to create and collect the synthetic corpus.' } else { 'Word COM is not registered. No document was created and no layout results were produced.' })
    }
}

function New-TextRun([string] $Text, [string] $Font, [double] $Size, [bool] $Bold = $false) {
    return [pscustomobject]@{ text=$Text; font=$Font; size=$Size; bold=$Bold }
}

function New-CorpusParagraph([string] $Id, [int] $Alignment, [double] $Left, [double] $Right, [double] $First, [int] $LineRule, [double] $LineSpacing, $Runs) {
    return [pscustomobject]@{ id=$Id; alignment=$Alignment; leftIndent=$Left; rightIndent=$Right; firstLineIndent=$First; lineRule=$LineRule; lineSpacing=$LineSpacing; runs=@($Runs) }
}

function Get-Corpus {
    $corpus = @()
    $runs = @(New-TextRun 'En français, les mots composés, les espaces insécables et les caractères accentués doivent rester lisibles lorsque Word replie cette phrase autour de la zone de texte définie.' 'Arial' 11)
    $corpus += New-CorpusParagraph 'fr-left-single' 0 0 0 0 0 12 $runs

    $runs = @()
    $runs += New-TextRun 'A centered English paragraph gives the reference a different measure; ' 'Times New Roman' 11
    $runs += New-TextRun 'mixed-size words change the available width' 'Arial' 16
    $runs += New-TextRun ' while punctuation, “smart quotes,” and an em dash remain part of the exact source text.' 'Times New Roman' 11
    $corpus += New-CorpusParagraph 'en-center-mixed' 1 18 18 0 0 12 $runs

    $runs = @(New-TextRun 'Alignement à droite et interligne un et demi : cette phrase française contient déjà plusieurs groupes de mots afin de produire plusieurs lignes physiques dans Word.' 'Arial' 11)
    $corpus += New-CorpusParagraph 'fr-right-15' 2 12 12 0 1 18 $runs

    $runs = @(New-TextRun 'Justified English text exercises a first-line indent and double spacing. A short final line checks that Word leaves the last line unexpanded.' 'Times New Roman' 12)
    $corpus += New-CorpusParagraph 'en-justify-double' 3 0 0 24 2 24 $runs

    $text = "Retrait suspendu et hauteur exacte : les lettres liées, le cœur, Noël, l’e-mail et une suite combinante e$([char]0x0301) testent les frontières du texte source."
    $runs = @(New-TextRun $text 'Arial' 11)
    $corpus += New-CorpusParagraph 'fr-hanging-exact' 0 24 0 -18 4 14 $runs

    $runs = @(New-TextRun 'A deliberately overlong token follows: pneumonoultramicroscopicsilicovolcanoconiosis; the surrounding words show where Word wraps before and after it.' 'Arial' 11)
    $corpus += New-CorpusParagraph 'en-long-token' 0 0 0 0 0 12 $runs

    $runs = @()
    $runs += New-TextRun 'Une phrase justifiée avec ' 'Times New Roman' 11
    $runs += New-TextRun 'un passage en gras de taille différente' 'Arial' 14 $true
    $runs += New-TextRun ' puis un retour à la taille courante. Les fins de ligne sont relevées depuis les plages Word.' 'Times New Roman' 11
    $corpus += New-CorpusParagraph 'fr-justify-mixed' 3 9 9 14 0 12 $runs

    $runs = @(New-TextRun 'A final indented English paragraph uses one and a half spacing and a right indent to exercise a narrower measure with reproducible source offsets.' 'Arial' 11)
    $corpus += New-CorpusParagraph 'en-1-5-space' 0 0 24 18 1 18 $runs
    return $corpus
}

function Add-SyntheticDocument($Word, [string] $Path, $Corpus) {
    $directory = Split-Path -Parent $Path
    if (-not (Test-Path -LiteralPath $directory)) {
        New-Item -ItemType Directory -Path $directory -Force | Out-Null
    }
    $document = $Word.Documents.Add()
    $document.TrackRevisions = $false
    $pageSetup = $document.Sections.Item(1).PageSetup
    $pageSetup.PaperSize = 7 # wdPaperA4
    $pageSetup.TopMargin = 54
    $pageSetup.BottomMargin = 54
    $pageSetup.LeftMargin = 54
    $pageSetup.RightMargin = 54

    $runManifest = @{}
    foreach ($item in $Corpus) {
        $text = ($item.runs | ForEach-Object { $_.text }) -join ''
        $start = [int] $document.Content.End - 1
        $insert = $document.Range($start, $start)
        $insert.InsertBefore($text)
        $paragraphRange = $document.Range($start, ($start + $text.Length))
        $paragraph = $paragraphRange.Paragraphs.Item(1)
        $format = $paragraph.Format
        $format.Alignment = [int] $item.alignment
        $format.LeftIndent = [double] $item.leftIndent
        $format.RightIndent = [double] $item.rightIndent
        $format.FirstLineIndent = [double] $item.firstLineIndent
        $format.LineSpacingRule = [int] $item.lineRule
        $format.LineSpacing = [double] $item.lineSpacing
        $format.SpaceBefore = 0
        $format.SpaceAfter = 6

        $offset = 0
        $runSpecs = @()
        foreach ($run in $item.runs) {
            $runStart = $start + $offset
            $runEnd = $runStart + $run.text.Length
            $runRange = $document.Range($runStart, $runEnd)
            $runRange.Font.Name = [string] $run.font
            $runRange.Font.Size = [single] $run.size
            if ($run.bold) { $runRange.Font.Bold = $true }
            $runSpecs += [pscustomobject]@{
                paragraphId = $item.id
                utf16Start = $offset
                utf16End = $offset + $run.text.Length
                requestedFont = [string] $run.font
                requestedSizePt = [double] $run.size
            }
            $offset += $run.text.Length
        }
        $runManifest[$item.id] = $runSpecs
        $paragraphRange.InsertParagraphAfter()
    }

    $document.SaveAs2([System.IO.Path]::GetFullPath($Path), 12) # wdFormatXMLDocument
    $document.Close(0)
    return $runManifest
}

function Convert-Utf16OffsetToUtf8($Text, [int] $Offset) {
    if ($Offset -lt 0 -or $Offset -gt $Text.Length) {
        throw "UTF-16 offset $Offset is outside text length $($Text.Length)."
    }
    return [System.Text.Encoding]::UTF8.GetByteCount($Text.Substring(0, $Offset))
}

function Assert-SyntheticOutputPaths([string] $SyntheticPath, [string] $JsonPath) {
    $repoRoot = Split-Path $PSScriptRoot -Parent
    $allowedRoot = [System.IO.Path]::GetFullPath((Join-Path $repoRoot 'tests/text-layout-reference/generated'))
    $allowedPrefix = $allowedRoot.TrimEnd([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
    $fullSyntheticPath = [System.IO.Path]::GetFullPath($SyntheticPath)
    $fullJsonPath = [System.IO.Path]::GetFullPath($JsonPath)
    if (-not $fullSyntheticPath.StartsWith($allowedPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Synthetic DOCX path must be inside the generated fixture directory: $allowedRoot"
    }
    if ([string]::Equals($fullSyntheticPath, $fullJsonPath, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw 'Synthetic DOCX and JSON output paths must be different.'
    }
    if (Test-Path -LiteralPath $fullSyntheticPath) {
        throw "Refusing to overwrite an existing synthetic DOCX: $fullSyntheticPath"
    }
    $artifactRoot = [System.IO.Path]::GetFullPath((Join-Path $repoRoot '_bmad-output/implementation-artifacts'))
    $artifactPrefix = $artifactRoot.TrimEnd([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
    if (-not ($fullJsonPath.StartsWith($allowedPrefix, [System.StringComparison]::OrdinalIgnoreCase) -or
              $fullJsonPath.StartsWith($artifactPrefix, [System.StringComparison]::OrdinalIgnoreCase))) {
        throw 'JSON output must be inside the generated fixture or implementation-artifact directory.'
    }
    if ([System.IO.Path]::GetExtension($fullJsonPath) -ne '.json') {
        throw 'JSON output must have a .json extension.'
    }
    if (Test-Path -LiteralPath $fullJsonPath) {
        throw "Refusing to overwrite an existing reference JSON: $fullJsonPath"
    }
    return $fullSyntheticPath
}

function Get-WordRangeLines($Document, $ParagraphStart, $ContentEnd, $ParagraphText) {
    $lines = @()
    $cursor = [int] $ParagraphStart
    while ($cursor -lt $ContentEnd) {
        $lineRange = $Document.Range($cursor, $cursor)
        $null = $lineRange.MoveEnd(5, 1) # wdLine: Word's rendered line unit
        $lineEnd = [Math]::Min([int] $lineRange.End, [int] $ContentEnd)
        if ($lineEnd -le $cursor) {
            throw "Word did not advance the physical line range at document position $cursor."
        }
        $localStart = $cursor - $ParagraphStart
        $localEnd = $lineEnd - $ParagraphStart
        $lineText = $ParagraphText.Substring($localStart, $localEnd - $localStart)
        $lineOrigin = $Document.Range($cursor, $cursor)
        $page = $lineOrigin.Information(3) # wdActiveEndPageNumber at line start
        $vertical = $lineOrigin.Information(6) # wdVerticalPositionRelativeToPage, points
        $horizontal = $lineOrigin.Information(5) # wdHorizontalPositionRelativeToPage, points
        $lines += [pscustomobject]@{
            lineIndex = $lines.Count
            text = $lineText
            utf16Start = $localStart
            utf16End = $localEnd
            utf8ByteStart = Convert-Utf16OffsetToUtf8 $ParagraphText $localStart
            utf8ByteEnd = Convert-Utf16OffsetToUtf8 $ParagraphText $localEnd
            wordDocumentUtf16Start = $cursor
            wordDocumentUtf16End = $lineEnd
            pageNumber = [int] $page
            topPt = $(if ([double] $vertical -ge 0) { [double] $vertical } else { $null })
            leftPt = $(if ([double] $horizontal -ge 0) { [double] $horizontal } else { $null })
        }
        $cursor = $lineEnd
    }
    if ($lines.Count -eq 0 -and $ParagraphText.Length -eq 0) {
        return @([pscustomobject]@{ lineIndex=0; text=''; utf16Start=0; utf16End=0; utf8ByteStart=0; utf8ByteEnd=0; wordDocumentUtf16Start=$ParagraphStart; wordDocumentUtf16End=$ParagraphStart; pageNumber=$null; topPt=$null; leftPt=$null })
    }
    return $lines
}

function Get-ReferenceResult($Word, [string] $Path, $Corpus, $RunManifest) {
    $document = $Word.Documents.Open([System.IO.Path]::GetFullPath($Path), $false, $true)
    try {
        $document.Repaginate()
        $paragraphs = @()
        for ($index = 0; $index -lt $Corpus.Count; $index++) {
            $fixture = $Corpus[$index]
            $paragraph = $document.Paragraphs.Item($index + 1)
            $range = $paragraph.Range
            $paragraphStart = [int] $range.Start
            $paragraphEnd = [int] $range.End
            $paragraphText = [string] $range.Text
            if ($paragraphText.EndsWith("`r")) { $paragraphText = $paragraphText.Substring(0, $paragraphText.Length - 1) }
            $contentEnd = $paragraphStart + $paragraphText.Length
            $lines = @(Get-WordRangeLines $document $paragraphStart $contentEnd $paragraphText)

            $actualRuns = @()
            foreach ($spec in $RunManifest[$fixture.id]) {
                $runRange = $document.Range(($paragraphStart + $spec.utf16Start), ($paragraphStart + $spec.utf16End))
                $actualRuns += [pscustomobject]@{
                    utf16Start = $spec.utf16Start
                    utf16End = $spec.utf16End
                    utf8ByteStart = Convert-Utf16OffsetToUtf8 $paragraphText $spec.utf16Start
                    utf8ByteEnd = Convert-Utf16OffsetToUtf8 $paragraphText $spec.utf16End
                    requestedFont = $spec.requestedFont
                    wordDeclaredFontName = [string] $runRange.Font.Name
                    requestedSizePt = $spec.requestedSizePt
                    wordFontSizePt = [double] $runRange.Font.Size
                }
            }
            $paragraphs += [pscustomobject]@{
                paragraphId = $fixture.id
                text = $paragraphText
                utf16Length = $paragraphText.Length
                utf8ByteLength = [System.Text.Encoding]::UTF8.GetByteCount($paragraphText)
                wordDocumentUtf16Start = $paragraphStart
                wordDocumentUtf16End = $contentEnd
                requestedParagraph = [ordered]@{
                    alignment = $fixture.alignment
                    leftIndentPt = $fixture.leftIndent
                    rightIndentPt = $fixture.rightIndent
                    firstLineIndentPt = $fixture.firstLineIndent
                    lineSpacingRule = $fixture.lineRule
                    lineSpacingPt = $fixture.lineSpacing
                }
                requestedAndWordFonts = $actualRuns
                lines = $lines
            }
        }
        $section = $document.Sections.Item(1).PageSetup
        return [ordered]@{
            schemaVersion = 1
            story = 'US-210'
            oracle = 'Microsoft Word desktop via Word.Application COM'
            fontIdentityPolicy = 'COM Font.Name is the declared formatting name, not proof of the physical font used after substitution. Treat line-wrap comparison as eligible for a 95% claim only when the compared rendering paths use identical font files.'
            capturedAtUtc = [DateTime]::UtcNow.ToString('o')
            wordVersion = [string] $Word.Version
            document = [ordered]@{
                path = [System.IO.Path]::GetFullPath($Path)
                generatedLocallyByThisScript = $true
                paperSize = [int] $section.PaperSize
                pageWidthPt = [double] $section.PageWidth
                pageHeightPt = [double] $section.PageHeight
                topMarginPt = [double] $section.TopMargin
                bottomMarginPt = [double] $section.BottomMargin
                leftMarginPt = [double] $section.LeftMargin
                rightMarginPt = [double] $section.RightMargin
                units = 'points (1/72 inch)'
            }
            lineBoundaryMethod = 'Word Range.MoveEnd(wdLine, 1), with page and physical coordinates from Range.Information; line offsets are paragraph-local UTF-16 code units plus UTF-8 byte offsets.'
            paragraphs = $paragraphs
        }
    }
    finally {
        $document.Close(0)
        [void][System.Runtime.InteropServices.Marshal]::ReleaseComObject($document)
    }
}

$wordType = Get-WordComType
if ($AvailabilityOnly) { $Mode = 'Availability' }
if ($Mode -eq 'Availability') {
    Write-JsonResult (Get-AvailabilityResult) ''
    exit 0
}
if ($null -eq $wordType) {
    Write-JsonResult (Get-AvailabilityResult) ''
    exit 0
}

$SyntheticDocx = Assert-SyntheticOutputPaths $SyntheticDocx $OutputJson

$word = $null
try {
    $word = New-Object -ComObject Word.Application
    $word.Visible = $false
    $word.DisplayAlerts = 0
    $word.ScreenUpdating = $false
    $corpus = @(Get-Corpus)
    $runManifest = Add-SyntheticDocument $word $SyntheticDocx $corpus
    $result = Get-ReferenceResult $word $SyntheticDocx $corpus $runManifest
    Write-JsonResult $result $OutputJson
}
finally {
    if ($null -ne $word) {
        $word.Quit(0)
        [void][System.Runtime.InteropServices.Marshal]::ReleaseComObject($word)
    }
    [GC]::Collect()
    [GC]::WaitForPendingFinalizers()
}
