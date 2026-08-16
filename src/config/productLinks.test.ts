import { describe, expect, it } from "vitest";
import {
  CCSWITCHMULTI_REPOSITORY_URL,
  CCSWITCHMULTI_RELEASES_URL,
} from "./productLinks";

describe("CCSwitchMulti product links", () => {
  it("keeps every public product destination on the Multi repository", () => {
    expect(CCSWITCHMULTI_REPOSITORY_URL).toBe(
      "https://github.com/BigStrongSun/ccswitchmulti",
    );
    expect(CCSWITCHMULTI_RELEASES_URL).toBe(
      "https://github.com/BigStrongSun/ccswitchmulti/releases",
    );
  });
});
