package de.timscho.config.core.model;

import com.fasterxml.jackson.annotation.JsonProperty;

/**
 * One file layer a rebuilding image watches, from {@code schema.reload.layers}.
 *
 * <p>The environment is deliberately not a constant: a process's environment is fixed for its
 * lifetime, so no image could truthfully claim to watch it.
 */
public enum ReloadLayer {
    /** The TOML layer at {@code <PREFIX>CONFIG}. */
    @JsonProperty("document")
    DOCUMENT,

    /** The directory of key-named files at {@code <PREFIX>SECRETS_DIR}. */
    @JsonProperty("secrets_dir")
    SECRETS_DIR,

    /** The files {@code <PREFIX><KEY>_FILE} variables name. */
    @JsonProperty("env_file")
    ENV_FILE
}
