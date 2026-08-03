Preview built with [VHS (PR#719)](https://github.com/charmbracelet/vhs/pull/719).

Reads are PacBio single-nucleus data from LongBench ([bioRxiv](https://www.biorxiv.org/content/10.1101/2025.09.11.675724v3), [GSE303762](https://ncbi.nlm.nih.gov/geo/query/acc.cgi?acc=GSE303762), [AWS S3](https://github.com/mritchielab/LongBench.io)):
```bash
# grab 1k reads from AWS S3
curl -sS "https://longbench-data.s3.amazonaws.com/raw/fastq/SN_PB.fastq.gz" \
  | zcat \
  | sed -n '57,4056p;4056q' > test.fastq

# build VHS with captions support
gh pr checkout 719
go build -o vhs .
./vhs demo.tape
```
