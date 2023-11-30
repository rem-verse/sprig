@{
  Rules=@{
    PSAvoidLongLines = @{
      Enable            = $true
      MaximumLineLength = 100
    }
    PSAvoidSemicolonsAsLineTerminators = @{
      Enable = $true
    }
    AvoidTrailingWhitespace = @{
      Enable = $true
    }
    PSUseConsistentIndentation = @{
      Enable = $true
      IndentationSize = 2
      PipelineIndentation = 'NoIndentation'
      Kind = 'tab'
    }
  }
}