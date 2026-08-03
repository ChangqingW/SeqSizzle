Output seqsizzle-demo.mp4
Output seqsizzle-demo.gif
Set Shell zsh
Set Theme "OneHalfDark"
Set FontSize 34
Set Framerate 24
Set Width 1920
Set Height 1080 
Set TypingSpeed 0.3

Set CaptionFontSize 64
Set OverlayFontSize 64
Set OverlayAlignment "top-right"

Hide
Type "export PATH=$HOME/SeqSizzle/target/release:$PATH"
Enter
Ctrl+L
Show

Overlay@3s "Start with built-in adapter sequences"
Type@50ms "seqsizzle test.fastq --adapter-3p"
Sleep 800ms
Enter
Sleep 800ms

CaptionOn
Overlay@3s "'o' to toggle options"
Type "o"
Sleep 800ms
Type "b"
Sleep 800ms
Type "b"
Overlay@3s "Mouse clicks can be enable with 'c'"
Sleep 800ms
Type "c"
Sleep 800ms
Type "o"
Sleep 800ms

Ctrl+D
Overlay@3s "scroll with up / down or VI keys\Npage up / down with Ctrl+U/D"
Down@150ms 9
Ctrl+D
Sleep 800ms

Overlay@3s "Click [patterns] or use '/' key to open search panel"
Type "/"
Sleep 800ms
Sleep 800ms
Overlay@3s "Click on (or use arrow keys to select) patterns to edit them"
Down@200ms 2
Sleep 800ms
Enter
Sleep 800ms
Type "/"
Sleep 800ms

Sleep 800ms
Type "/"
Overlay@3s "Set sequence / color / edit-distance"
Sleep 800ms
Right
Sleep 200ms
Backspace@100ms 5
Type@100ms "Red"
Sleep 100ms
Tab
Sleep 100ms
Backspace
Sleep 100ms
Type "4"
Sleep 500ms
Enter
Sleep 500ms
Type "/"
Sleep 200ms

Down@200ms 7
Sleep 800ms
Type "q"
CaptionOff

Overlay@3s "Find enriched sequences if you don't know what adapters to look for"
Type@50ms "seqsizzle test.fastq enrich -o enrich.csv"
Sleep 800ms
Sleep 800ms
Enter
Wait@10s
Type@50ms "head enrich.csv"
Sleep 800ms
Sleep 800ms
Enter
Sleep 800ms
Sleep 800ms

Overlay@3s "Summarize the reads patterns"
Type@50ms "seqsizzle test.fastq --adapter-3p summarize | head"
Sleep 800ms
Sleep 800ms
Enter
Wait@10s
Sleep 800ms
