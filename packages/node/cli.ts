#!/usr/bin/env node
import { runMain } from 'citty'

import { createBenchmarkCommand } from './scripts/benchmark-cli'

runMain(createBenchmarkCommand())
